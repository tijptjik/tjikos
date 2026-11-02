import makeWASocket, { DisconnectReason, proto, type WASocket } from 'baileys';
import pino from 'pino';
import { authentication } from './auth.ts';
import { Sender } from './sender.ts';
import { WhatsoverError, type Status, type Store } from './types.ts';

type SocketFactory = typeof makeWASocket;

export class WhatsAppSession {
    readonly sender: Sender;
    private socket?: WASocket;
    private current: Status = { state: 'stopped' };
    private qr?: string;
    private stopped = true;
    private retry?: ReturnType<typeof setTimeout>;
    private attempts = 0;
    private authWrites: Promise<void> = Promise.resolve();
    private auth?: Awaited<ReturnType<typeof authentication>>;
    private opening?: Promise<void>;

    constructor(private store: Store, private factory: SocketFactory = makeWASocket) {
        this.sender = new Sender(store, {
            connected: () => this.current.state === 'connected',
            send: async (to, text, messageId) => {
                const message = await this.socket!.sendMessage(to, { text, linkPreview: null }, { messageId });
                if (!message) throw new Error('No message returned by transport');
                await this.authWrites;
                if (this.current.state === 'failed') throw new Error('Session persistence failed');
            },
        });
    }

    status(): Status { return { ...this.current }; }
    pairing(): { qr?: string; state: Status['state'] } { return { qr: this.qr, state: this.current.state }; }

    async start(): Promise<void> {
        if (!this.stopped) return;
        this.stopped = false;
        this.current = { state: 'starting' };
        try {
            this.auth = await authentication(this.store, () => this.fail());
            await this.auth.save();
            await this.connect();
        } catch (error) {
            this.stopped = true;
            this.current = { state: 'failed' };
            throw error;
        }
    }

    private connect(): Promise<void> {
        this.opening = this.open();
        return this.opening;
    }

    private async open(): Promise<void> {
        await this.authWrites;
        if (this.stopped) return;
        const socket = this.factory({
            auth: this.auth!.state,
            logger: pino({ level: 'silent' }),
            browser: ['Whatsover', 'Chrome', '1.0.0'],
            markOnlineOnConnect: false,
            syncFullHistory: false,
            shouldSyncHistoryMessage: () => false,
            connectTimeoutMs: 30000,
            defaultQueryTimeoutMs: 30000,
            getMessage: async key => {
                const text = key.id ? await this.store.message(key.id) : undefined;
                return text === undefined ? undefined : { conversation: text };
            },
        });
        this.socket = socket;
        socket.ev.on('creds.update', () => {
            if (socket !== this.socket) return;
            this.authWrites = this.authWrites.then(() => this.auth!.save()).catch(() => {
                this.fail();
            });
        });
        socket.ev.on('connection.update', update => {
            if (socket !== this.socket || this.stopped) return;
            if (update.qr) {
                this.qr = update.qr;
                this.current = { state: 'pairing' };
            }
            if (update.connection === 'open') {
                this.qr = undefined;
                this.attempts = 0;
                this.current = { state: 'connected', account: socket.user?.id };
            }
            if (update.connection === 'close') {
                clearTimeout(this.retry);
                this.qr = undefined;
                const code = (update.lastDisconnect?.error as { output?: { statusCode?: number } })?.output?.statusCode;
                const terminal = [DisconnectReason.loggedOut, DisconnectReason.badSession,
                    DisconnectReason.connectionReplaced, DisconnectReason.forbidden, DisconnectReason.multideviceMismatch];
                if (code !== undefined && terminal.includes(code)) {
                    this.stopped = true;
                    this.current = { state: code === DisconnectReason.loggedOut ? 'logged_out' : 'failed' };
                    return;
                }
                this.current = { state: 'reconnecting' };
                const delay = code === DisconnectReason.restartRequired ? 0 : Math.min(30000, 1000 * 2 ** Math.min(this.attempts++, 5));
                this.retry = setTimeout(() => { void this.connect().catch(() => this.fail()); }, delay);
            }
        });
    }

    private fail(): void {
        this.stopped = true;
        this.qr = undefined;
        this.current = { state: 'failed' };
        clearTimeout(this.retry);
        void this.socket?.end(undefined).catch(() => {});
    }

    async groups() {
        if (this.current.state !== 'connected') throw new WhatsoverError('not_connected', 'WhatsApp is not connected.', 503);
        return Object.values(await this.socket!.groupFetchAllParticipating()).map(group => ({ id: group.id, name: group.subject, size: group.size, isCommunity: Boolean(group.isCommunity), isAnnouncement: Boolean(group.isCommunityAnnounce), parentId: group.linkedParent || null }));
    }

    async stop(): Promise<void> {
        this.stopped = true;
        this.current = { state: 'stopped' };
        clearTimeout(this.retry);
        // Finish accepted sends before closing the transport and store.
        await this.sender.drain();
        await this.opening?.catch(() => {});
        await this.socket?.end(undefined);
        await this.authWrites;
        this.qr = undefined;
        this.current = { state: 'stopped' };
    }
}
