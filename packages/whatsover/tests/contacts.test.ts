import { afterEach, expect, test } from 'bun:test';
import { mkdtempSync, rmSync } from 'node:fs';
import { SQLiteStore } from '../src/sqlite.ts';
import { saveContacts } from '../src/members.ts';
import { validateContact, updateContact } from '../src/contacts.ts';

const cleanup: (() => Promise<void>)[] = [];
afterEach(async () => { for (const action of cleanup.splice(0)) await action(); });
async function fixture() {
    const directory = mkdtempSync('/tmp/whatsover-contact-');
    const store = new SQLiteStore(directory);
    cleanup.push(async () => { await store.close(); rmSync(directory, { recursive: true, force: true }); });
    await saveContacts(store, [{ id: '85255550123@s.whatsapp.net', name: 'Synthetic', lid: '1234567@lid' }]);
    return store;
}
const input = { to: '+85255550123', firstName: 'Synthetic', lastName: 'Example', expectedName: 'Synthetic' };

test('contact writes contain names only and request primary addressbook save', async () => {
    const store = await fixture();
    const calls: unknown[] = [];
    const result = await updateContact(store, validateContact(input), async (contact, jid) => { calls.push({ contact, jid }); });
    expect(calls).toEqual([{ jid: '85255550123@s.whatsapp.net', contact: {
        fullName: 'Synthetic Example', firstName: 'Synthetic', pnJid: '85255550123@s.whatsapp.net', lidJid: '1234567@lid', saveOnPrimaryAddressbook: true,
    } }]);
    expect(result.state).toBe('saved');
    expect(JSON.parse((await store.readAuth('contact:1234567@lid'))!).name).toBe('Synthetic Example');
});

test('a changed identity blocks the write', async () => {
    const store = await fixture();
    let calls = 0;
    await expect(updateContact(store, validateContact({ ...input, expectedName: 'Another Person' }), async () => { calls++; })).rejects.toMatchObject({ code: 'contact_conflict' });
    expect(calls).toBe(0);
});

test('failed server writes do not mark the local contact saved', async () => {
    const store = await fixture();
    await expect(updateContact(store, validateContact(input), async () => { throw new Error('Rejected'); })).rejects.toThrow('Rejected');
    expect(JSON.parse((await store.readAuth('contact:85255550123@s.whatsapp.net'))!).name).toBe('Synthetic');
});

test('descriptions, placeholders and non-individual destinations cannot become contact names', () => {
    expect(() => validateContact({ ...input, description: 'A relationship; Country' })).toThrow();
    expect(() => validateContact({ ...input, lastName: 'XXX' })).toThrow();
    expect(() => validateContact({ ...input, to: '123456789@g.us' })).toThrow();
    expect(() => validateContact({ ...input, firstName: 'Line\nBreak' })).toThrow();
    expect(validateContact({ ...input, lastName: '' }).lastName).toBe('');
});
