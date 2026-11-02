#!/usr/bin/env bun
import { parseArgs } from 'node:util';
import { chmodSync } from 'node:fs';
import { join } from 'node:path';
import qrcode from 'qrcode-terminal';
import { Whatsover } from './client.ts';
import { WhatsAppSession } from './session.ts';
import { SQLiteStore, stateDirectory } from './sqlite.ts';
import { handler } from './api.ts';
import { WhatsoverError } from './types.ts';

const help = `whatsover — scriptable WhatsApp linked device

  whatsover serve [--state-dir PATH] [--duration SECONDS]
                  [--port PORT --host HOST]
  whatsover login                          Display QR and wait for pairing
  whatsover status                         Connection status (JSON)
  whatsover groups                         Group names and JIDs (JSON)
  whatsover send --to +852… --text 'Hello' --id stable-job-id
  whatsover send --to GROUP_JID --stdin --id stable-job-id
  whatsover result REQUEST_ID              Inspect a previous send (JSON)

Client options: --socket PATH, or --url https://… with WHATSOVER_TOKEN.
Default socket: $WHATSOVER_STATE_DIR/service.sock
Default state: $XDG_STATE_HOME/whatsover or ~/.local/state/whatsover
HTTP serve requires WHATSOVER_TOKEN (at least 32 characters).
Start serve first; login and send talk to that one service.
Stopping preserves the login. No command sends anything implicitly.
`;

function positive(value: string, name: string, maximum = Number.MAX_SAFE_INTEGER): number {
    const number = Number(value);
    if (!Number.isSafeInteger(number) || number <= 0 || number > maximum) throw new Error(`${name} must be a positive integer up to ${maximum}.`);
    return number;
}

export async function main(args = process.argv.slice(2)): Promise<void> {
    const command = args[0];
    if (!command || command === '--help' || command === 'help') { console.log(help); return; }
    const common = { socket: { type: 'string' }, url: { type: 'string' } } as const;
    const commands = {
        serve: { 'state-dir': { type: 'string' }, duration: { type: 'string' }, port: { type: 'string' }, host: { type: 'string' } },
        login: common, status: common, groups: common, result: common,
        send: { ...common, to: { type: 'string' }, text: { type: 'string' }, stdin: { type: 'boolean' }, id: { type: 'string' } },
    } as const;
    if (!(command in commands)) throw new Error(`Unknown command: ${command}. Run whatsover --help.`);
    const { values, positionals } = parseArgs({ args: args.slice(1), options: commands[command as keyof typeof commands], allowPositionals: command === 'result', strict: true });
    const options = values as Record<string, string | boolean | undefined>;
    const string = (key: string) => options[key] as string | undefined;
    if (command === 'serve') {
        if (string('host') && !string('port')) throw new Error('--host requires --port.');
        const port = string('port') ? positive(string('port')!, 'port', 65535) : undefined;
        const duration = string('duration') ? positive(string('duration')!, 'duration', 604800) : undefined;
        const token = process.env.WHATSOVER_TOKEN;
        if (port && (!token || token.length < 32)) throw new Error('HTTP serve requires WHATSOVER_TOKEN with at least 32 characters.');
        process.umask(0o077);
        const store = new SQLiteStore(string('state-dir'));
        const session = new WhatsAppSession(store);
        let server: ReturnType<typeof Bun.serve>;
        try {
            const options = { fetch: handler(session, store, port ? token : undefined), maxRequestBodySize: 128 * 1024 };
            server = port
                ? Bun.serve({ ...options, port, hostname: string('host') || '127.0.0.1', idleTimeout: 60 })
                : Bun.serve({ ...options, unix: join(store.directory, 'service.sock') });
            if (!port) chmodSync(join(store.directory, 'service.sock'), 0o600);
            await session.start();
        } catch (error) {
            await server!?.stop(true);
            await session.stop();
            await store.close();
            throw error;
        }
        console.error('whatsover service ready; use whatsover login to link your account.');
        let stopping = false;
        let timer: ReturnType<typeof setTimeout> | undefined;
        const stop = async () => {
            if (stopping) return;
            stopping = true;
            clearTimeout(timer);
            const deadline = setTimeout(() => process.exit(1), 45000);
            try {
                await server.stop(false);
                await session.stop();
                await store.close();
                clearTimeout(deadline);
                process.exit(0);
            } catch {
                console.error('whatsover shutdown failed; pending sends will remain uncertain.');
                process.exit(1);
            }
        };
        process.on('SIGINT', () => { void stop(); });
        process.on('SIGTERM', () => { void stop(); });
        if (duration) timer = setTimeout(() => { void stop(); }, duration * 1000);
        return;
    }

    const url = string('url') || process.env.WHATSOVER_URL;
    const socket = string('socket');
    if (socket && url) throw new Error('Choose --socket or --url/WHATSOVER_URL, not both.');
    const client = new Whatsover({ url, socket: url ? undefined : socket || join(stateDirectory(), 'service.sock'), token: process.env.WHATSOVER_TOKEN });
    const output = (data: unknown) => console.log(JSON.stringify(data));
    switch (command) {
        case 'status': output(await client.status()); break;
        case 'groups': output(await client.groups()); break;
        case 'result':
            if (positionals.length !== 1) throw new Error('Usage: whatsover result REQUEST_ID');
            output(await client.result(positionals[0]));
            break;
        case 'send': {
            if (Boolean(options.stdin) === (string('text') !== undefined)) throw new Error('Provide exactly one of --text or --stdin.');
            const text = options.stdin ? await Bun.stdin.text() : string('text');
            output(await client.send({ to: string('to')!, text: text!, id: string('id')! }));
            break;
        }
        case 'login': {
            let previous = '';
            for (let attempts = 0; attempts < 120; attempts++) {
                const pairing = await client.pairing();
                if (pairing.state === 'connected') { output(await client.status()); return; }
                if (['logged_out', 'failed', 'stopped'].includes(pairing.state)) throw new Error(`Session is ${pairing.state}. See README for recovery.`);
                if (pairing.qr && pairing.qr !== previous) {
                    previous = pairing.qr;
                    console.error('WhatsApp → Settings → Linked devices → Link a device');
                    qrcode.generate(pairing.qr, { small: true }, code => console.error(code));
                }
                await Bun.sleep(1000);
            }
            throw new Error('Pairing timed out. Run login again for a fresh QR code.');
        }
    }
}

if (import.meta.main) {
    main().catch(error => {
        console.error(JSON.stringify({
            error: error instanceof WhatsoverError ? error.code : 'command_failed',
            message: error instanceof Error ? error.message : 'Command failed.',
            ...(error instanceof WhatsoverError && error.result ? { result: error.result } : {}),
        }));
        process.exitCode = 1;
    });
}
