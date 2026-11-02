import { afterEach, expect, test } from 'bun:test';
import { mkdtempSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { SQLiteStore } from '../src/sqlite.ts';
import { Sender } from '../src/sender.ts';
import { handler, type SessionAPI } from '../src/api.ts';
import { Whatsover } from '../src/client.ts';

const cleanup: (() => unknown | Promise<unknown>)[] = [];
afterEach(async () => { for (const fn of cleanup.splice(0).reverse()) await fn(); });
function fixture(token?: string) {
    const directory = mkdtempSync('/tmp/whatsover-api-');
    const store = new SQLiteStore(directory);
    cleanup.push(async () => { await store.close(); rmSync(directory, { recursive: true, force: true }); });
    let calls = 0;
    const session: SessionAPI = {
        status: () => ({ state: 'connected', account: 'synthetic' }),
        pairing: () => ({ state: 'pairing', qr: 'synthetic-qr' }),
        groups: async () => [{ id: '120363000000000000@g.us', name: 'Synthetic group' }],
        sender: new Sender(store, { connected: () => true, send: async () => { calls++; } }),
    };
    return { directory, store, session, handle: handler(session, store, token), calls: () => calls };
}

test('HTTP authentication covers QR, status, sends, groups and results', async () => {
    const app = fixture('synthetic-api-token');
    for (const path of ['/v1/qr', '/v1/status', '/v1/groups', '/v1/messages/id', '/v1/messages']) {
        expect((await app.handle(new Request(`http://localhost${path}`))).status).toBe(401);
    }
    const response = await app.handle(new Request('http://localhost/v1/qr', { headers: { authorization: 'Bearer synthetic-api-token' } }));
    expect(response.status).toBe(200);
    expect(response.headers.get('cache-control')).toBe('no-store');
    expect(await response.json()).toMatchObject({ qr: 'synthetic-qr' });
});

test('rejects malformed input and browser origins without sending', async () => {
    const app = fixture();
    const post = (body: string, headers = { 'content-type': 'application/json' }) => app.handle(new Request('http://localhost/v1/messages', { method: 'POST', body, headers }));
    expect((await post('{')).status).toBe(400);
    expect((await post('{}')).status).toBe(400);
    expect((await post('{}', { 'content-type': 'text/plain' })).status).toBe(415);
    expect((await post(' '.repeat(129 * 1024))).status).toBe(413);
    expect((await app.handle(new Request('http://localhost/v1/qr', { headers: { origin: 'https://example.com' } }))).status).toBe(403);
    expect(app.calls()).toBe(0);
});

test('Unix socket library and CLI round trip without WhatsApp network access', async () => {
    const app = fixture();
    const socket = join(app.directory, 'service.sock');
    const server = Bun.serve({ unix: socket, fetch: app.handle });
    cleanup.push(() => server.stop(true));
    const client = new Whatsover({ socket });
    expect((await client.status()).state).toBe('connected');
    expect(await client.groups()).toHaveLength(1);
    const request = { id: 'synthetic-cli-job', to: '+85255550123', text: 'Synthetic CLI fixture' };
    const child = Bun.spawn([process.execPath, join(import.meta.dir, '../src/cli.ts'), 'send', '--socket', socket,
        '--to', request.to, '--id', request.id, '--stdin'], { stdin: new Blob([request.text]), stdout: 'pipe', stderr: 'pipe', env: { ...process.env, WHATSOVER_URL: '' } });
    const output = await new Response(child.stdout).text();
    expect(await child.exited).toBe(0);
    expect(JSON.parse(output)).toMatchObject({ id: request.id, state: 'submitted' });
    expect(await client.send(request)).toEqual(JSON.parse(output));
    expect(app.calls()).toBe(1);
    expect(await client.result(request.id)).toEqual(JSON.parse(output));
    expect(output).not.toContain(request.text);
    expect((await client.result(request.id)) as any).not.toHaveProperty('fingerprint');
});

test('HTTP library authenticates and surfaces uncertain results', async () => {
    const app = fixture('synthetic-api-token');
    const server = Bun.serve({ hostname: '127.0.0.1', port: 0, fetch: app.handle });
    cleanup.push(() => server.stop(true));
    const url = `http://127.0.0.1:${server.port}`;
    const client = new Whatsover({ url, token: 'synthetic-api-token' });
    expect((await client.status()).state).toBe('connected');
    await expect(new Whatsover({ url, token: 'wrong' }).pairing()).rejects.toMatchObject({ code: 'unauthorized' });
    const request = { id: 'synthetic-uncertain', to: '+85255550123', text: 'Fixture' };
    const first = await client.send(request);
    await app.store.update(request.id, 'uncertain');
    await expect(client.send(request)).rejects.toMatchObject({ result: { messageId: first.messageId, state: 'uncertain' } });
    expect(app.calls()).toBe(1);
});

test('remote clients require HTTPS, tokens and explicit transport', () => {
    expect(() => new Whatsover({})).toThrow();
    expect(() => new Whatsover({ socket: '/tmp/test', url: 'https://example.com', token: 'fixture' })).toThrow();
    expect(() => new Whatsover({ url: 'http://example.com', token: 'fixture' })).toThrow('HTTPS');
    expect(() => new Whatsover({ url: 'https://example.com' })).toThrow('token');
    expect(() => new Whatsover({ url: 'https://user:password@example.com', token: 'fixture' })).toThrow();
});
