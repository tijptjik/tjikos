export type ConnectionState = 'starting' | 'pairing' | 'connected' | 'reconnecting' | 'logged_out' | 'failed' | 'stopped';
export interface Status { state: ConnectionState; account?: string; }
export interface SendRequest { to: string; text: string; id: string; }
export interface SendResult {
    id: string;
    to: string;
    messageId: string;
    state: 'pending' | 'submitted' | 'uncertain';
    createdAt: number;
}
export interface OutboxEntry extends SendResult { fingerprint: string; text: string; }

// Implementations must persist writes before resolving and atomically claim IDs.
// Only one running WhatsApp client may own a given store.
export interface Store {
    readAuth(key: string): Promise<string | null>;
    writeAuth(entries: [string, string | null][]): Promise<void>;
    claim(entry: OutboxEntry): Promise<OutboxEntry>;
    result(id: string): Promise<OutboxEntry | null>;
    update(id: string, state: SendResult['state']): Promise<void>;
    message(messageId: string): Promise<string | undefined>;
    close(): Promise<void>;
}

export class WhatsoverError extends Error {
    constructor(public code: string, message: string, public status = 400, public result?: SendResult) {
        super(message);
        this.name = 'WhatsoverError';
    }
}

export function recipient(input: string): string {
    if (/^\+?[1-9]\d{6,14}$/.test(input)) return `${input.replace(/^\+/, '')}@s.whatsapp.net`;
    if (/^\d{7,15}@s\.whatsapp\.net$/.test(input) || /^\d+(?:-\d+)?@g\.us$/.test(input) || /^\d+@lid$/.test(input)) return input;
    throw new WhatsoverError('invalid_recipient', 'Use an international phone number (+852…), a group JID, or a LID.');
}

export function validateSend(input: unknown): SendRequest {
    if (!input || typeof input !== 'object') throw new WhatsoverError('invalid_request', 'Expected a JSON object.');
    const { to, text, id } = input as Record<string, unknown>;
    if (typeof to !== 'string' || typeof text !== 'string' || !text.trim() || text.length > 16000) {
        throw new WhatsoverError('invalid_request', 'Provide to and nonempty text (maximum 16000 characters).');
    }
    if (typeof id !== 'string' || !/^[a-zA-Z0-9_.:-]{1,128}$/.test(id)) {
        throw new WhatsoverError('invalid_id', 'Provide a stable request id (1–128 letters, numbers, dots, colons, underscores or hyphens).');
    }
    return { to: recipient(to), text, id };
}

export function publicResult(entry: OutboxEntry): SendResult {
    const { id, to, messageId, state, createdAt } = entry;
    return { id, to, messageId, state, createdAt };
}
