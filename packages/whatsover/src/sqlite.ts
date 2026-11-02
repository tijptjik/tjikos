import { Database } from 'bun:sqlite';
import { chmodSync, existsSync, mkdirSync, realpathSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { homedir } from 'node:os';
import type { OutboxEntry, SendResult, Store } from './types.ts';

export function stateDirectory(): string {
    return process.env.WHATSOVER_STATE_DIR || join(process.env.XDG_STATE_HOME || join(homedir(), '.local/state'), 'whatsover');
}

export function privateDirectory(path: string): string {
    const absolute = resolve(path);
    let existing = absolute;
    while (!existsSync(existing)) existing = dirname(existing);
    const check = (directory: string) => {
        for (let current = realpathSync(directory); ; current = dirname(current)) {
            const git = join(current, '.git');
            if (existsSync(join(git, 'HEAD')) || (existsSync(git) && statSync(git).isFile())) {
                throw new Error('Whatsover state must be outside a Git repository.');
            }
            if (dirname(current) === current) break;
        }
    };
    check(existing);
    mkdirSync(absolute, { recursive: true, mode: 0o700 });
    check(absolute);
    chmodSync(absolute, 0o700);
    return realpathSync(absolute);
}

/** Local single-owner store. OS SQLite locks are released even after SIGKILL. */
export class SQLiteStore implements Store {
    private db: Database;
    private owner: Database;
    readonly directory: string;

    constructor(directory = stateDirectory()) {
        this.directory = privateDirectory(directory);
        const ownerPath = join(this.directory, 'owner.sqlite');
        this.owner = new Database(ownerPath, { create: true });
        chmodSync(ownerPath, 0o600);
        try {
            this.owner.exec('PRAGMA busy_timeout=0; BEGIN EXCLUSIVE; CREATE TABLE IF NOT EXISTS owner (id INTEGER)');
        } catch {
            this.owner.close();
            throw new Error('Another whatsover process owns this state directory. Use its API or stop it first.');
        }
        try {
            const databasePath = join(this.directory, 'session.sqlite');
            this.db = new Database(databasePath, { create: true });
            chmodSync(databasePath, 0o600);
            this.db.exec(`PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
                CREATE TABLE IF NOT EXISTS auth (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS outbox (
                    id TEXT PRIMARY KEY, message_id TEXT UNIQUE NOT NULL, data TEXT NOT NULL
                );`);
            // An interrupted send cannot safely be retried automatically.
            this.db.exec(`UPDATE outbox SET data=json_set(data, '$.state', 'uncertain') WHERE json_extract(data, '$.state')='pending'`);
        } catch (error) {
            this.owner.close();
            throw error;
        }
    }

    async readAuth(key: string): Promise<string | null> {
        return (this.db.query('SELECT value FROM auth WHERE key=?').get(key) as { value: string } | null)?.value ?? null;
    }

    async writeAuth(entries: [string, string | null][]): Promise<void> {
        this.db.transaction(() => {
            for (const [key, value] of entries) {
                if (value === null) this.db.query('DELETE FROM auth WHERE key=?').run(key);
                else this.db.query('INSERT OR REPLACE INTO auth VALUES (?, ?)').run(key, value);
            }
        })();
    }

    async claim(entry: OutboxEntry): Promise<OutboxEntry> {
        this.db.query('INSERT OR IGNORE INTO outbox VALUES (?, ?, ?)').run(entry.id, entry.messageId, JSON.stringify(entry));
        return (await this.result(entry.id))!;
    }

    async result(id: string): Promise<OutboxEntry | null> {
        const row = this.db.query('SELECT data FROM outbox WHERE id=?').get(id) as { data: string } | null;
        return row ? JSON.parse(row.data) : null;
    }

    async update(id: string, state: SendResult['state']): Promise<void> {
        this.db.query("UPDATE outbox SET data=json_set(data, '$.state', ?) WHERE id=?").run(state, id);
    }

    async message(messageId: string): Promise<string | undefined> {
        const row = this.db.query('SELECT data FROM outbox WHERE message_id=?').get(messageId) as { data: string } | null;
        return row ? (JSON.parse(row.data) as OutboxEntry).text : undefined;
    }

    async close(): Promise<void> {
        this.db.close();
        this.owner.close();
    }
}
