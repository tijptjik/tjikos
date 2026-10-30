// Exercise the real background script against a synthetic Firefox API.
const vm = require('node:vm');
const fs = require('node:fs');
const assert = require('node:assert/strict');
const updates = [];
const createdTabs = [];
const historyQueries = [];
const deletedHistory = [];
const tabUpdates = [];
const removed = [];
const closedWindows = [];
const windows = [
    {id: 1, titlePreface: 'Work: ', incognito: false, tabs: [{id: 11, title: 'Same page', active: true, url: 'https://example.org/read', favIconUrl: 'https://example.org/favicon.ico'}, {id: 12, title: 'Pinned', pinned: true}]},
    {id: 2, incognito: true, tabs: [{id: 22, title: 'Private'}]}
];
const context = vm.createContext({URL, setTimeout() {}, browser: {
    runtime: {connectNative: () => ({onMessage: {addListener() {}}, onDisconnect: {addListener() {}}})},
    windows: {
        getAll: async () => windows,
        remove: async id => closedWindows.push(id),
        get: async id => windows.find(w => w.id === id),
        update: async (id, value) => updates.push([id, value])
    },
    history: {
        deleteUrl: async value => deletedHistory.push(value),
        search: async query => { historyQueries.push(query); return [{id:'h1', title:'Earlier page', url:'https://example.org/read', lastVisitTime:1234, visitCount:3}, {id:'bad',url:'javascript:alert(1)'}, {id:'file',url:'file:///private'}]; },
        getVisits: async ({url}) => url==='https://example.org/read' ? [{visitId:'v1'}] : []
    },
    tabs: {
        create: async value => {createdTabs.push(value); return {id:13, windowId:value.windowId};},
        get: async id => ({id, windowId: [11,12,13].includes(id) ? 1 : 3, pinned: id === 12, url: 'https://example.org/read', title: 'Same page'}),
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
    const history = await run({op:'history',text:'earlier'});
    assert.equal(history.history.length,1);
    assert.equal(history.history[0].visitCount,3);
    assert.equal(historyQueries[0].maxResults,100);
    assert.equal(historyQueries[0].startTime,0);
    await assert.rejects(run({op:'history',text:'x'.repeat(513)}),/Invalid/);
    await assert.rejects(run({op:'open-history',url:'javascript:alert(1)'}),/HTTP/);
    await assert.rejects(run({op:'open-history',url:'https://example.org/deleted'}),/removed/);
    const reopened = await run({op:'open-history',url:'https://example.org/read',marker:'[history] '});
    assert.equal(reopened.window,1);
    assert.equal(createdTabs[0].windowId,1);
    assert.equal(createdTabs.length,1);
    await assert.rejects(run({op:'delete-history',url:'file:///private'}),/HTTP/);
    assert.equal(deletedHistory.length,0);
    assert.equal((await run({op:'delete-history',url:'https://example.org/read'})).deleted,true);
    assert.equal(JSON.stringify(deletedHistory),JSON.stringify([{url:'https://example.org/read'}]));
    assert.equal(createdTabs.length,1);
    console.log('PASS: private exclusion, stale tabs, tab activation, title prefix restoration');
})().catch(error => { console.error(error); process.exitCode = 1; });
