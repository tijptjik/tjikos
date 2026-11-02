import { BufferJSON, initAuthCreds, proto, type AuthenticationState } from 'baileys';
import type { Store } from './types.ts';

export async function authentication(store: Store, failed: () => void = () => {}) {
    const saved = await store.readAuth('creds');
    const creds = saved ? JSON.parse(saved, BufferJSON.reviver) : initAuthCreds();
    const state: AuthenticationState = {
        creds,
        keys: {
            async get(type, ids) {
                const entries = await Promise.all(ids.map(async id => {
                    const stored = await store.readAuth(JSON.stringify([type, id]));
                    let value = stored ? JSON.parse(stored, BufferJSON.reviver) : undefined;
                    if (type === 'app-state-sync-key' && value) value = proto.Message.AppStateSyncKeyData.fromObject(value);
                    return [id, value];
                }));
                return Object.fromEntries(entries);
            },
            async set(data) {
                const entries: [string, string | null][] = [];
                for (const [type, values] of Object.entries(data)) {
                    for (const [id, value] of Object.entries(values ?? {})) {
                        entries.push([JSON.stringify([type, id]), value == null ? null : JSON.stringify(value, BufferJSON.replacer)]);
                    }
                }
                try { await store.writeAuth(entries); }
                catch (error) { failed(); throw error; }
            },
        },
    };
    return {
        state,
        save: () => store.writeAuth([['creds', JSON.stringify(creds, BufferJSON.replacer)]]),
    };
}
