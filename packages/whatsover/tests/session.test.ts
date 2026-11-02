import { afterEach, expect, test } from 'bun:test';
import { EventEmitter } from 'node:events';
import { mkdtempSync, rmSync } from 'node:fs';
import { DisconnectReason, proto, type WASocket } from 'baileys';
import { SQLiteStore } from '../src/sqlite.ts';
import { WhatsAppSession } from '../src/session.ts';

const cleanup: (() => Promise<void>)[] = [];
afterEach(async () => { for (const fn of cleanup.splice(0)) await fn(); });
async function fixture() {
    const directory = mkdtempSync('/tmp/whatsover-session-');
    const store = new SQLiteStore(directory);
    const sockets: { events: EventEmitter; ended: boolean }[] = [];
    const session = new WhatsAppSession(store, config => {
        const record = { events: new EventEmitter(), ended: false };
        sockets.push(record);
        expect(config.syncFullHistory).toBe(false);
        expect(config.markOnlineOnConnect).toBe(false);
        expect(config.shouldSyncHistoryMessage!({ syncType: proto.HistorySync.HistorySyncType.PUSH_NAME })).toBe(true);
        expect(config.shouldSyncHistoryMessage!({ syncType: proto.HistorySync.HistorySyncType.FULL })).toBe(false);
        expect(config.shouldSyncHistoryMessage!({ syncType: proto.HistorySync.HistorySyncType.RECENT })).toBe(false);
        return {
            ev: record.events, user: { id: 'synthetic-account' },
            end: async () => { record.ended = true; },
            sendMessage: async () => ({ key: { id: 'fixture' } }),
        } as unknown as WASocket;
    });
    cleanup.push(async () => { await session.stop(); await store.close(); rmSync(directory, { recursive: true, force: true }); });
    await session.start();
    return { session, store, sockets, events: sockets[0].events };
}

test('QR is cleared after pairing and after disconnect', async () => {
    const { session, events } = await fixture();
    events.emit('connection.update', { qr: 'synthetic-qr' });
    expect(session.pairing()).toEqual({ state: 'pairing', qr: 'synthetic-qr' });
    events.emit('connection.update', { connection: 'open' });
    expect(session.pairing()).toEqual({ state: 'connected', qr: undefined });
    expect(session.status().account).toBe('synthetic-account');
});

test('profile-name snapshots are cached without importing chat history', async () => {
    const { store, events } = await fixture();
    events.emit('messaging-history.set', { syncType: proto.HistorySync.HistorySyncType.PUSH_NAME, contacts: [{ id: '85255550123@s.whatsapp.net', notify: 'Synthetic Profile' }] });
    events.emit('messaging-history.set', { syncType: proto.HistorySync.HistorySyncType.FULL, contacts: [{ id: '85255550124@s.whatsapp.net', name: 'Not imported' }] });
    await Bun.sleep(10);
    expect(JSON.parse((await store.readAuth('contact:85255550123@s.whatsapp.net'))!).notify).toBe('Synthetic Profile');
    expect(await store.readAuth('contact:85255550124@s.whatsapp.net')).toBeNull();
});

test('pairing restart opens a replacement socket; stale events are ignored', async () => {
    const { session, events, sockets } = await fixture();
    events.emit('connection.update', { connection: 'close', lastDisconnect: { error: { output: { statusCode: DisconnectReason.restartRequired } } } });
    await Bun.sleep(20);
    expect(sockets).toHaveLength(2);
    sockets[1].events.emit('connection.update', { connection: 'open' });
    events.emit('connection.update', { qr: 'stale-qr' });
    expect(session.pairing()).toEqual({ state: 'connected', qr: undefined });
});

test('logout stops reconnects and never silently creates a new identity', async () => {
    const { session, events, sockets } = await fixture();
    events.emit('connection.update', { connection: 'close', lastDisconnect: { error: { output: { statusCode: DisconnectReason.loggedOut } } } });
    expect(session.status().state).toBe('logged_out');
    await Bun.sleep(20);
    expect(sockets).toHaveLength(1);
});

test('credential persistence failure stops the connection', async () => {
    const { session, events, store, sockets } = await fixture();
    store.writeAuth = async () => { throw new Error('Synthetic disk failure'); };
    events.emit('creds.update', {});
    await Bun.sleep(10);
    expect(session.status().state).toBe('failed');
    expect(sockets[0].ended).toBe(true);
});

test('shutdown cancels reconnects and preserves credentials', async () => {
    const { session, store, events, sockets } = await fixture();
    events.emit('connection.update', { connection: 'close' });
    await session.stop();
    expect(session.status().state).toBe('stopped');
    expect(await store.readAuth('creds')).not.toBeNull();
    expect(sockets[0].ended).toBe(true);
});
