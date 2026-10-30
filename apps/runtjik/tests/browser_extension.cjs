// Exercise the real background script against a synthetic Firefox API.
const vm = require('node:vm');
const fs = require('node:fs');
const assert = require('node:assert/strict');
const updates = [];
const tabUpdates = [];
const removed = [];
const closedWindows = [];
const windows = [
    {id: 1, titlePreface: 'Work: ', incognito: false, tabs: [{id: 11, title: 'Same page', active: true, url: 'https://example.org/read', favIconUrl: 'https://example.org/favicon.ico'}, {id: 12, title: 'Pinned', pinned: true}]},
    {id: 2, incognito: true, tabs: [{id: 22, title: 'Private'}]}
];
const context = vm.createContext({setTimeout() {}, browser: {
    runtime: {connectNative: () => ({onMessage: {addListener() {}}, onDisconnect: {addListener() {}}})},
    windows: {
        getAll: async () => windows,
        remove: async id => closedWindows.push(id),
        get: async id => windows.find(w => w.id === id),
        update: async (id, value) => updates.push([id, value])
    },
    tabs: {
        get: async id => ({id, windowId: [11,12].includes(id) ? 1 : 3, pinned: id === 12, url: 'https://example.org/read', title: 'Same page'}),
        remove: async id => removed.push(id),
        update: async (id, value) => tabUpdates.push([id, value])
    }
}});
vm.runInContext(fs.readFileSync(require('node:path').join(__dirname, '../browser-extension/background.js'), 'utf8'), context);
async function run(value) {
    context.input = value;
    return JSON.parse(JSON.stringify(await vm.runInContext('handle(input)', context)));
}
(async () => {
    assert.equal((await run({op: 'list'})).windows.length, 1);
    await assert.rejects(run({op: 'focus', window: 2}), /Private/);
    await assert.rejects(run({op: 'focus', window: 1, tab: 99}), /moved/);
    await run({op: 'focus', window: 1, tab: 11, marker: '[unique] '});
    assert.equal(tabUpdates[0][0], 11);
    assert.equal(updates[0][1].titlePreface, '[unique] ');
    await run({op: 'release', window: 1});
    assert.equal(updates[1][1].titlePreface, 'Work: ');
    const listing = await run({op: 'list'});
    assert.equal(listing.windows[0].tabs.length, 1);
    assert.equal(listing.windows[0].tabs[0].favicon, 'https://example.org/favicon.ico');
    assert.equal((await run({op: 'details', window: 1, tab: 11})).tab.url, 'https://example.org/read');
    await assert.rejects(run({op: 'close', window: 1, tab: 12}), /Pinned/);
    await assert.rejects(run({op: 'close', window: 1, tab: 11, url: 'https://example.org/old'}), /navigated/);
    assert.equal(removed.length, 0);
    await run({op: 'close', window: 1, tab: 11, url: 'https://example.org/read'});
    assert.deepEqual(removed, [11]);
    await assert.rejects(run({op: 'close', window: 2, tab: null}), /Private/);
    assert.equal(closedWindows.length, 0);
    await run({op: 'close', window: 1, tab: null});
    assert.deepEqual(closedWindows, [1]);
    console.log('PASS: private exclusion, stale tabs, tab activation, title prefix restoration');
})().catch(error => { console.error(error); process.exitCode = 1; });
