const vm = require('node:vm');
const fs = require('node:fs');
const assert = require('node:assert/strict');
const {webcrypto} = require('node:crypto');
const path = require('node:path');
const stored = {}, saves = [], notices = [];
let fail = false, reject = false, listener, opened = 0;
const tab = {windowId: 7, id: 2, title: 'Synthetic page', url: 'https://example.test/read'};
const context = vm.createContext({URL, crypto: webcrypto, structuredClone, browser: {
    tabs: {query: async () => [tab]}, windows: {getAll: async () => [{id: tab.windowId}]},
    storage: {local: {get: async key => key === null ? {...stored} : {[key]: stored[key]}, set: async values => Object.assign(stored, structuredClone(values)), remove: async key => delete stored[key]}},
    runtime: {getURL: p => 'moz-extension://tasktjik/' + p, sendMessage: async () => {}, onMessage: {addListener: fn => listener = fn}, sendNativeMessage: async (host, value) => {
        assert.equal(host, 'org.tjikos.tasktjik');
        saves.push(structuredClone(value));
        if (fail) throw Error('Lost reply');
        if (reject) return {error: 'Invalid title', rejected: true};
        return {saved: true};
    }},
    commands: {onCommand: {addListener() {}}}, browserAction: {onClicked: {addListener() {}}},
    sidebarAction: {open: () => { opened++; return Promise.resolve(); }},
    notifications: {create: async value => notices.push(value.message)}
}});
vm.runInContext(fs.readFileSync(path.join(__dirname, '../extension/background.js'), 'utf8'), context);
const run = code => vm.runInContext(code, context);
const sender = {url: 'moz-extension://tasktjik/panel.html'};
(async () => {
    await Promise.all([run("command('save-inbox')"), run("command('save-inbox')")]);
    assert.equal(saves.length, 1);
    assert.equal(saves[0].fields.group, 'All');
    assert.equal(Object.keys(stored).length, 0);
    await run("command('save-inbox')");
    assert.equal(saves.length, 1, 'Repeated shortcut must not duplicate the page');
    tab.url = 'https://example.test/second';
    fail = true;
    await run("command('save-inbox')");
    const draft = stored['capture:7'];
    assert(draft.submitted);
    fail = false;
    const result = await listener({op: 'save', windowId: 7, request_id: draft.request_id, fields: draft.submitted}, sender);
    assert.equal(result.saved, true);
    assert.equal(saves.at(-1).request_id, saves.at(-2).request_id);
    assert.deepEqual(saves.at(-1).fields, saves.at(-2).fields);
    tab.url = 'https://example.test/third';
    await run("command('save-panel')");
    assert.equal(opened, 1);
    const pending = stored['capture:7'];
    tab.url = 'https://example.test/fourth';
    await run("command('save-inbox')");
    assert.equal(stored['capture:7'].request_id, pending.request_id);
    assert.match(notices.at(-1), /pending/);
    const count = saves.length;
    tab.incognito = true;
    await run("command('save-inbox')");
    assert.equal(saves.length, count);
    tab.incognito = false; tab.url = 'file:///private';
    await run("command('save-inbox')");
    assert.equal(saves.length, count);
    assert.equal(await listener({op: 'save'}, {url: 'https://example.test'}), undefined);
    reject = true;
    const rejection = await listener({op: 'save', windowId: 7, request_id: pending.request_id, fields: {...pending.fields, title: ''}}, sender);
    assert.equal(rejection.rejected, true);
    assert.equal(stored['capture:7'].submitted, null);
    reject = false;
    tab.windowId = 99; tab.url = pending.fields.reference;
    await run("command('save-panel')");
    assert.equal(stored['capture:99'].request_id, pending.request_id, 'Restart recovery must preserve request identity');
    assert.equal(stored['capture:7'], undefined);
    console.log('PASS: Inbox capture, duplicate suppression, durable retries, pending draft preservation, private/internal exclusion');
})().catch(error => { console.error(error); process.exitCode = 1; });
