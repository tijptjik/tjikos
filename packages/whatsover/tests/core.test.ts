import { afterEach, describe, expect, test } from 'bun:test';
import { mkdtempSync, rmSync, statSync, mkdirSync, symlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { SQLiteStore, privateDirectory } from '../src/sqlite.ts';
import { authentication } from '../src/auth.ts';
import { Sender } from '../src/sender.ts';
import { recipient, type OutboxEntry } from '../src/types.ts';

const directories: string[] = [];
const stores: SQLiteStore[] = [];
function temporary() { const dir = mkdtempSync('/tmp/whatsover-test-'); directories.push(dir); return dir; }
function store() { const value = new SQLiteStore(temporary()); stores.push(value); return value; }
afterEach(async () => {
    for (const value of stores.splice(0)) await value.close();
    for (const dir of directories.splice(0)) rmSync(dir, { recursive: true, force: true });
});
const request = { id: 'synthetic-job-1', to: '+85255550123', text: 'Synthetic fixture only' };

describe('durable sends', () => {
    test('concurrent duplicate calls send once, persist across restart, and reject conflicting reuse', async () => {
        const db = store();
        let sent = 0;
        const transport = { connected: () => true, send: async () => { sent++; await Bun.sleep(5); } };
        const sender = new Sender(db, transport);
        const [first, second] = await Promise.all([sender.send(request), sender.send(request)]);
        expect(first).toEqual(second);
        expect(first.state).toBe('submitted');
        expect(sent).toBe(1);
        expect(first.to).toBe('85255550123@s.whatsapp.net');
        await expect(sender.send({ ...request, text: 'Different body' })).rejects.toMatchObject({ code: 'id_conflict' });
        await db.close(); stores.pop();
        const restarted = new SQLiteStore(db.directory); stores.push(restarted);
        expect(await new Sender(restarted, transport).send(request)).toEqual(first);
        expect(sent).toBe(1);
    });

    test('ambiguous network errors preserve the request and never replay it', async () => {
        const db = store();
        let sent = 0;
        const sender = new Sender(db, { connected: () => true, send: async () => { sent++; throw new Error('lost acknowledgement'); } });
        await expect(sender.send(request)).rejects.toMatchObject({ code: 'send_uncertain', result: { id: request.id, state: 'uncertain' } });
        expect((await sender.send(request)).state).toBe('uncertain');
        expect(sent).toBe(1);
    });

    test('offline and invalid requests do not reserve IDs', async () => {
        const db = store();
        const sender = new Sender(db, { connected: () => false, send: async () => {} });
        await expect(sender.send(request)).rejects.toMatchObject({ code: 'not_connected' });
        expect(await db.result(request.id)).toBeNull();
        expect(() => sender.send({ ...request, id: undefined })).toThrow();
        expect(() => sender.send({ ...request, text: ' ' })).toThrow();
    });

    test('interrupted pending sends become uncertain on startup', async () => {
        const db = store();
        await db.claim({ ...request, to: recipient(request.to), state: 'pending', messageId: 'synthetic-id', createdAt: 1, fingerprint: 'fixture' });
        await db.close(); stores.pop();
        const restarted = new SQLiteStore(db.directory); stores.push(restarted);
        expect((await restarted.result(request.id))?.state).toBe('uncertain');
    });

    test('send intent exists before transport starts', async () => {
        const db = store();
        const sender = new Sender(db, { connected: () => true, send: async (_to, _text, messageId) => {
            expect((await db.result(request.id))?.state).toBe('pending');
            expect(await db.message(messageId)).toBe(request.text);
        } });
        await sender.send(request);
    });
});

describe('session storage', () => {
    test('credentials, binary signal keys and key deletions survive reopening', async () => {
        const db = store();
        const auth = await authentication(db);
        await auth.save();
        await auth.state.keys.set({ 'pre-key': { 'fixture/key': { private: Buffer.from([1, 2]), public: Buffer.from([3, 4]) } } });
        const identity = auth.state.creds.signedIdentityKey.public;
        await db.close(); stores.pop();
        const restarted = new SQLiteStore(db.directory); stores.push(restarted);
        const loaded = await authentication(restarted);
        expect(loaded.state.creds.signedIdentityKey.public).toEqual(identity);
        expect((await loaded.state.keys.get('pre-key', ['fixture/key']))['fixture/key']!.private).toEqual(Buffer.from([1, 2]));
        await loaded.state.keys.set({ 'pre-key': { 'fixture/key': null } });
        expect((await loaded.state.keys.get('pre-key', ['fixture/key']))['fixture/key']).toBeUndefined();
        expect(statSync(db.directory).mode & 0o777).toBe(0o700);
        expect(statSync(join(db.directory, 'session.sqlite')).mode & 0o777).toBe(0o600);
    });

    test('a second process cannot own the same store', async () => {
        const db = store();
        const child = Bun.spawn([process.execPath, '-e', `import { SQLiteStore } from ${JSON.stringify(join(import.meta.dir, '../src/sqlite.ts'))}; new SQLiteStore(process.argv[1]);`, db.directory], { stdout: 'pipe', stderr: 'pipe' });
        expect(await child.exited).not.toBe(0);
        expect(await new Response(child.stderr).text()).toContain('Another whatsover process');
    });

    test('crashed owner releases the OS lock', async () => {
        const directory = temporary();
        const child = Bun.spawn([process.execPath, '-e', `import { SQLiteStore } from ${JSON.stringify(join(import.meta.dir, '../src/sqlite.ts'))}; new SQLiteStore(process.argv[1]); console.log('ready'); setInterval(()=>{}, 1000);`, directory], { stdout: 'pipe', stderr: 'pipe' });
        const reader = child.stdout.getReader();
        expect(new TextDecoder().decode((await reader.read()).value)).toContain('ready');
        child.kill('SIGKILL'); await child.exited;
        const recovered = new SQLiteStore(directory); stores.push(recovered);
        expect(await recovered.readAuth('creds')).toBeNull();
    });

    test('rejects state under repositories, including symlink aliases', () => {
        const dir = temporary();
        mkdirSync(join(dir, 'repo/.git'), { recursive: true });
        writeFileSync(join(dir, 'repo/.git/HEAD'), 'ref: refs/heads/main\n');
        symlinkSync(join(dir, 'repo'), join(dir, 'alias'));
        expect(() => privateDirectory(join(dir, 'repo/private'))).toThrow('outside a Git repository');
        expect(() => privateDirectory(join(dir, 'alias/private'))).toThrow('outside a Git repository');
    });
});

test('recipient validation requires explicit destinations', () => {
    expect(recipient('120363000000000000@g.us')).toBe('120363000000000000@g.us');
    expect(recipient('123456789@lid')).toBe('123456789@lid');
    for (const value of ['Alice', '123', '+852 5555 0123', 'status@broadcast', '123@s.whatsapp.net/evil']) expect(() => recipient(value)).toThrow();
});
