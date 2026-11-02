import { createHash, randomBytes } from 'node:crypto';
import { publicResult, validateSend, WhatsoverError, type SendResult, type Store } from './types.ts';

export interface Transport {
    connected(): boolean;
    send(to: string, text: string, messageId: string): Promise<void>;
}

/** A durable intent is written before network I/O; uncertain sends are never replayed. */
export class Sender {
    private tail: Promise<unknown> = Promise.resolve();
    constructor(private store: Store, private transport: Transport) {}

    send(input: unknown): Promise<SendResult> {
        const request = validateSend(input);
        const job = this.tail.then(async () => {
            const fingerprint = createHash('sha256').update(JSON.stringify([request.to, request.text])).digest('hex');
            const existing = await this.store.result(request.id);
            if (existing) {
                if (existing.fingerprint !== fingerprint) throw new WhatsoverError('id_conflict', 'This request id belongs to a different message.', 409);
                return publicResult(existing);
            }
            if (!this.transport.connected()) throw new WhatsoverError('not_connected', 'WhatsApp is not connected. Check status or run login.', 503);
            const candidate = {
                ...request, fingerprint, messageId: `3EB0${randomBytes(18).toString('hex').toUpperCase()}`,
                state: 'pending' as const, createdAt: Date.now(),
            };
            const claimed = await this.store.claim(candidate);
            if (claimed.messageId !== candidate.messageId) {
                if (claimed.fingerprint !== fingerprint) throw new WhatsoverError('id_conflict', 'This request id belongs to a different message.', 409);
                return publicResult(claimed);
            }
            try {
                await this.transport.send(request.to, request.text, claimed.messageId);
                await this.store.update(request.id, 'submitted');
            } catch {
                // The server may have accepted the message before the connection failed.
                await this.store.update(request.id, 'uncertain');
                throw new WhatsoverError('send_uncertain', 'Send outcome is uncertain. Check the chat; do not retry under a new id automatically.', 409,
                    { ...publicResult(claimed), state: 'uncertain' });
            }
            return { ...publicResult(claimed), state: 'submitted' as const };
        });
        this.tail = job.catch(() => {});
        return job;
    }

    async drain(): Promise<void> { await this.tail; }
}
