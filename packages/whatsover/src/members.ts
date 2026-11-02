import { jidNormalizedUser, type Contact, type GroupMetadata } from 'baileys';
import { parsePhoneNumberFromString } from 'libphonenumber-js/max';
import type { Store } from './types.ts';

export interface Member {
    first_name: string | null;
    last_name: string | null;
    country: string | null;
    phone_number: string | null;
    display_name: string | null;
    saved_contact_name: string | null;
    profile_name: string | null;
    name_source: 'saved_contact' | 'profile_name' | 'verified_business' | 'unavailable';
    name_split: 'unverified_display_name_split' | 'unavailable';
    country_source: 'phone_number_region' | 'unavailable';
    country_code: string | null;
    whatsapp_id: string;
    role: string;
}

export interface Membership {
    group: { id: string; name: string; is_community: boolean; is_announcement: boolean; parent_id: string | null; reported_size: number | null };
    fetched_at: string;
    contact_sync: 'complete' | 'unavailable';
    members: Member[];
}

const contactKey = (id: string) => `contact:${jidNormalizedUser(id)}`;

export async function saveContacts(store: Store, contacts: Partial<Contact>[]): Promise<void> {
    for (const contact of contacts) {
        if (!contact.id) continue;
        const ids = new Set([contact.id, contact.lid, contact.phoneNumber].filter((id): id is string => Boolean(id)).map(jidNormalizedUser));
        let previous: Partial<Contact> = {};
        for (const id of ids) {
            const saved = await store.readAuth(contactKey(id));
            if (saved) previous = { ...previous, ...JSON.parse(saved) };
        }
        const merged = { ...previous, ...Object.fromEntries(Object.entries(contact).filter(([, value]) => value !== undefined)) };
        for (const id of [merged.id, merged.lid, merged.phoneNumber]) if (id) ids.add(jidNormalizedUser(id));
        await store.writeAuth([...ids].map(id => [contactKey(id), JSON.stringify(merged)]));
    }
}

export function memberRow(id: string, pn: string | null, contact: Partial<Contact>, role = 'member'): Member {
    const digits = pn ? jidNormalizedUser(pn).split('@')[0] : null;
    const phone = digits && /^[1-9]\d{6,14}$/.test(digits) ? `+${digits}` : null;
    const parsed = phone ? parsePhoneNumberFromString(phone) : undefined;
    const code = parsed?.country ?? null;
    const name = contact.name || contact.notify || contact.verifiedName || null;
    const parts = name?.trim().split(/\s+/) ?? [];
    return {
        first_name: parts[0] || null,
        last_name: parts.length > 1 ? parts.slice(1).join(' ') : null,
        country: code ? new Intl.DisplayNames(['en'], { type: 'region' }).of(code) ?? code : null,
        phone_number: phone,
        display_name: name,
        saved_contact_name: contact.name || null,
        profile_name: contact.notify || null,
        name_source: contact.name ? 'saved_contact' : contact.notify ? 'profile_name' : contact.verifiedName ? 'verified_business' : 'unavailable',
        name_split: name ? 'unverified_display_name_split' : 'unavailable',
        country_source: code ? 'phone_number_region' : 'unavailable',
        country_code: code,
        whatsapp_id: id,
        role,
    };
}

export async function membership(metadata: GroupMetadata, store: Store, lookup: (lid: string) => Promise<string | null>, synced: boolean): Promise<Membership> {
    const members: Member[] = [];
    for (const participant of metadata.participants) {
        let phone = participant.phoneNumber || (participant.id.endsWith('@s.whatsapp.net') ? participant.id : null);
        if (!phone && participant.id.endsWith('@lid')) phone = await lookup(participant.id);
        let contact: Partial<Contact> = participant;
        for (const id of [participant.id, participant.lid, phone]) {
            if (!id) continue;
            const saved = await store.readAuth(contactKey(id));
            if (saved) contact = { ...contact, ...JSON.parse(saved) };
        }
        phone ||= contact.phoneNumber || null;
        members.push(memberRow(participant.id, phone, contact, participant.admin || 'member'));
    }
    return {
        group: { id: metadata.id, name: metadata.subject, is_community: Boolean(metadata.isCommunity), is_announcement: Boolean(metadata.isCommunityAnnounce), parent_id: metadata.linkedParent || null, reported_size: metadata.size ?? null },
        fetched_at: new Date().toISOString(), contact_sync: synced ? 'complete' : 'unavailable', members,
    };
}
