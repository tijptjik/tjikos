import { afterEach, expect, test } from 'bun:test';
import { mkdtempSync, rmSync } from 'node:fs';
import type { GroupMetadata } from 'baileys';
import { SQLiteStore } from '../src/sqlite.ts';
import { memberRow, membership, saveContacts } from '../src/members.ts';

const cleanup: (() => Promise<void>)[] = [];
afterEach(async () => { for (const action of cleanup.splice(0)) await action(); });
function fixture() {
    const directory = mkdtempSync('/tmp/whatsover-members-');
    const store = new SQLiteStore(directory);
    cleanup.push(async () => { await store.close(); rmSync(directory, { recursive: true, force: true }); });
    return store;
}

test('phone region and display-name splits are explicitly marked as inferred', () => {
    const row = memberRow('fixture@lid', '85255550123@s.whatsapp.net', { name: 'Synthetic van Example' });
    expect(row.phone_number).toBe('+85255550123');
    expect(row.country_code).toBe('HK');
    expect(row.country_source).toBe('phone_number_region');
    expect(row.first_name).toBe('Synthetic');
    expect(row.last_name).toBe('van Example');
    expect(row.name_split).toBe('unverified_display_name_split');
});

test('a LID is never fabricated into a phone number; missing names stay null', async () => {
    const store = fixture();
    const result = await membership({ id: '123@g.us', subject: 'Fixture', participants: [{ id: '123456789@lid' }] } as GroupMetadata, store, async () => null, false);
    expect(result.members[0]).toMatchObject({ phone_number: null, country: null, first_name: null, last_name: null, display_name: null });
    expect(result.contact_sync).toBe('unavailable');
});

test('saved contacts resolve across PN/LID and survive partial updates', async () => {
    const store = fixture();
    await saveContacts(store, [{ id: '85255550123@s.whatsapp.net', lid: '123456789@lid', name: 'Synthetic Person', phoneNumber: '85255550123@s.whatsapp.net' }]);
    await saveContacts(store, [{ id: '123456789@lid', notify: 'Alias' }]);
    const result = await membership({ id: '123@g.us', subject: 'Fixture', isCommunity: true, participants: [{ id: '123456789@lid', admin: 'admin' }] } as GroupMetadata, store, async () => '85255550123@s.whatsapp.net', true);
    expect(result.members[0]).toMatchObject({ first_name: 'Synthetic', last_name: 'Person', name_source: 'saved_contact', role: 'admin' });
    expect(result.group.is_community).toBe(true);
    expect(result.members[0].profile_name).toBe('Alias');
    expect(JSON.parse((await store.readAuth('contact:85255550123@s.whatsapp.net'))!).notify).toBe('Alias');
});

test('saved names and profile names remain separate matching inputs', () => {
    const row = memberRow('fixture@lid', null, { name: 'Synthetic Saved', notify: 'S. Profile' });
    expect(row.display_name).toBe('Synthetic Saved');
    expect(row.saved_contact_name).toBe('Synthetic Saved');
    expect(row.profile_name).toBe('S. Profile');
    expect(memberRow('fixture@lid', null, { notify: 'S. Profile' }).name_source).toBe('profile_name');
});

test('device suffix is removed without losing the actual phone number', () => {
    expect(memberRow('fixture', '85255550123:74@s.whatsapp.net', { notify: 'Synthetic' }).phone_number).toBe('+85255550123');
});
