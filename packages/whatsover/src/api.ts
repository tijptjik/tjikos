import { timingSafeEqual } from 'node:crypto';
import { publicResult, WhatsoverError, type SendResult, type Status, type Store } from './types.ts';
import type { Membership } from './members.ts';
import type { ContactResult } from './contacts.ts';

export interface SessionAPI {
    status(): Status;
    pairing(): { qr?: string; state: Status['state'] };
    sender: { send(input: unknown): Promise<SendResult> };
    groups(): Promise<{ id: string; name: string }[]>;
    members?(id: string): Promise<Membership>;
    saveContact?(input: unknown): Promise<ContactResult>;
}

export function handler(session: SessionAPI, store: Store, token?: string) {
    return async (request: Request): Promise<Response> => {
        const json = (value: unknown, status = 200) => Response.json(value, {
            status, headers: { 'cache-control': 'no-store', 'x-content-type-options': 'nosniff' },
        });
        if (token) {
            const actual = Buffer.from(request.headers.get('authorization') || '');
            const expected = Buffer.from(`Bearer ${token}`);
            if (actual.length !== expected.length || !timingSafeEqual(actual, expected)) return json({ error: 'unauthorized' }, 401);
        }
        // This is a script API, not a cross-origin browser endpoint.
        if (request.headers.has('origin')) return json({ error: 'browser_origin_rejected' }, 403);
        try {
            const path = new URL(request.url).pathname;
            if (request.method === 'GET' && path === '/v1/status') return json(session.status());
            if (request.method === 'GET' && path === '/v1/qr') return json(session.pairing());
            if (request.method === 'GET' && path === '/v1/groups') return json(await session.groups());
            const memberPath = path.match(/^\/v1\/groups\/([^/]+)\/members$/);
            if (request.method === 'GET' && memberPath && session.members) return json(await session.members(decodeURIComponent(memberPath[1])));
            if (request.method === 'GET' && path.startsWith('/v1/messages/')) {
                const record = await store.result(decodeURIComponent(path.slice('/v1/messages/'.length)));
                return record ? json(publicResult(record)) : json({ error: 'not_found' }, 404);
            }
            if (request.method === 'POST' && (path === '/v1/messages' || (path === '/v1/contacts' && session.saveContact))) {
                if (!request.headers.get('content-type')?.startsWith('application/json')) throw new WhatsoverError('invalid_content_type', 'Use application/json.', 415);
                // Bun.serve additionally limits the body before buffering it.
                const text = await request.text();
                if (Buffer.byteLength(text) > 128 * 1024) throw new WhatsoverError('too_large', 'Request body exceeds 128 KiB.', 413);
                let input: unknown;
                try { input = JSON.parse(text); } catch { throw new WhatsoverError('invalid_json', 'Invalid JSON.'); }
                if (path === '/v1/contacts') return json(await session.saveContact!(input));
                const result = await session.sender.send(input);
                return json(result, result.state === 'submitted' ? 200 : 409);
            }
            return json({ error: 'not_found' }, 404);
        } catch (error) {
            if (error instanceof WhatsoverError) return json({ error: error.code, message: error.message, result: error.result }, error.status);
            return json({ error: 'internal_error', message: 'Operation failed. Check service status.' }, 500);
        }
    };
}
