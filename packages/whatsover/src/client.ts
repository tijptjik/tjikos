import { WhatsoverError, type SendRequest, type SendResult, type Status } from './types.ts';

export interface ClientOptions { socket?: string; url?: string; token?: string; timeoutMs?: number; }

/** Script client; use one shared service for all callers. Socket transport requires Bun. */
export class Whatsover {
    private base: string;
    constructor(private options: ClientOptions) {
        if (Boolean(options.socket) === Boolean(options.url)) throw new Error('Provide exactly one of socket or url.');
        const url = new URL(options.url || 'http://localhost');
        if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.search || url.hash || url.pathname !== '/') {
            throw new Error('Use a plain HTTP(S) origin without credentials, path, query or fragment.');
        }
        if (url.protocol === 'http:' && !['localhost', '127.0.0.1', '[::1]'].includes(url.hostname)) {
            throw new Error('Remote service URLs must use HTTPS.');
        }
        if (options.url && !options.token) throw new Error('HTTP connections require WHATSOVER_TOKEN or a token option.');
        this.base = url.origin;
    }

    private async request<T>(path: string, data?: unknown): Promise<T> {
        const response = await fetch(`${this.base}${path}`, {
            ...(this.options.socket ? { unix: this.options.socket } : {}),
            method: data === undefined ? 'GET' : 'POST',
            headers: { ...(this.options.token ? { authorization: `Bearer ${this.options.token}` } : {}), 'content-type': 'application/json' },
            body: data === undefined ? undefined : JSON.stringify(data),
            signal: AbortSignal.timeout(this.options.timeoutMs ?? 45000),
            redirect: 'error',
        });
        const result = await response.json() as any;
        if (!response.ok) throw new WhatsoverError(result.error || 'request_failed', result.message || `Service returned ${response.status}.`, response.status, result.result || (result.id ? result : undefined));
        return result as T;
    }

    status() { return this.request<Status>('/v1/status'); }
    pairing() { return this.request<{ qr?: string; state: Status['state'] }>('/v1/qr'); }
    groups() { return this.request<{ id: string; name: string }[]>('/v1/groups'); }
    send(request: SendRequest) { return this.request<SendResult>('/v1/messages', request); }
    result(id: string) { return this.request<SendResult>(`/v1/messages/${encodeURIComponent(id)}`); }
}
