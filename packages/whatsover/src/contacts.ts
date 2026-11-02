import { jidNormalizedUser } from 'baileys';
import type { Store } from './types.ts';
import { recipient, WhatsoverError } from './types.ts';
import { saveContacts } from './members.ts';

export interface ContactUpdate { to: string; firstName: string; lastName: string; expectedName: string; }
export interface ContactResult { to: string; firstName: string; lastName: string; fullName: string; state: 'saved'; }

export function validateContact(input: unknown): ContactUpdate {
    if (!input || typeof input !== 'object') throw new WhatsoverError('invalid_contact', 'Expected a contact object.');
    const value = input as Record<string, unknown>;
    const allowed = new Set(['to', 'firstName', 'lastName', 'expectedName']);
    if (Object.keys(value).some(key => !allowed.has(key))) throw new WhatsoverError('invalid_contact', 'Contact updates accept only to, firstName, lastName and expectedName. Keep descriptions in the export.');
    for (const field of ['to', 'firstName', 'lastName', 'expectedName']) {
        if (typeof value[field] !== 'string' || (value[field] as string).length > 200 || /[\r\n\x00-\x1f]/.test(value[field] as string)) throw new WhatsoverError('invalid_contact', `Invalid ${field}.`);
    }
    const to = recipient(value.to as string);
    if (!to.endsWith('@s.whatsapp.net')) throw new WhatsoverError('invalid_contact', 'Contact saves require a phone number, not a group or LID.');
    const firstName = (value.firstName as string).trim();
    const lastName = (value.lastName as string).trim();
    if (!firstName || /\bXXX\b/i.test(`${firstName} ${lastName}`)) throw new WhatsoverError('invalid_contact', 'Supply a real name; unknown surname placeholders must remain empty.');
    return { to, firstName, lastName, expectedName: value.expectedName as string };
}

/** Guarded contact-only write. The transport resolves after the server accepts the patch. */
export async function updateContact(store: Store, request: ContactUpdate, write: (contact: { fullName: string; firstName: string; pnJid: string; lidJid?: string; saveOnPrimaryAddressbook: boolean }, jid: string) => Promise<void>): Promise<ContactResult> {
    const saved = await store.readAuth(`contact:${jidNormalizedUser(request.to)}`);
    const previous = saved ? JSON.parse(saved) : {};
    const currentName = previous.name || previous.notify || previous.verifiedName || '';
    const fullName = [request.firstName, request.lastName].filter(Boolean).join(' ');
    if (currentName !== request.expectedName && currentName !== fullName) {
        throw new WhatsoverError('contact_conflict', 'Contact name changed since matching. Refresh and review before saving.', 409);
    }
    const lid = previous.lid || (previous.id?.endsWith('@lid') ? previous.id : undefined);
    await write({ fullName, firstName: request.firstName, pnJid: request.to, lidJid: lid, saveOnPrimaryAddressbook: true }, request.to);
    await saveContacts(store, [{ id: request.to, phoneNumber: request.to, lid, name: fullName }]);
    return { to: request.to, firstName: request.firstName, lastName: request.lastName, fullName, state: 'saved' };
}
