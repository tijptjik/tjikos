// Run the unchanged submitted background script with controlled native ports and timers.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '../browser-extension/background.js'), 'utf8');

function boot() {
    const ports = [];
    const timers = [];
    const updates = [];
    let finishSearch;
    const context = vm.createContext({URL, setTimeout(callback, delay) { timers.push({callback, delay}); }, browser: {
        runtime: {connectNative(name) {
            assert.equal(name, 'org.tjikos.runtjik_browser');
            const port = {sent: [], onMessage: {addListener(fn) { port.receive = fn; }},
                onDisconnect: {addListener(fn) { port.disconnect = fn; }},
                postMessage(value) { this.sent.push(JSON.parse(JSON.stringify(value))); }};
            ports.push(port);
            return port;
        }},
        history: {search: () => new Promise(resolve => { finishSearch = resolve; })},
        windows: {
            get: async id => ({id, incognito: false, titlePreface: 'Work: '}),
            update: async (id, value) => updates.push([id, value]),
            getAll: async () => []
        }
    }});
    vm.runInContext(source, context);
    return {ports, timers, updates, finishSearch: () => finishSearch([])};
}

(async () => {
    const app = boot();
    assert.equal(app.ports.length, 1, 'Connect on browser startup without a launcher request');
    const first = app.ports[0];
    await first.receive({id: 'focus', op: 'focus', window: 1, marker: '[synthetic] '});
    assert.equal(app.updates[0][1].titlePreface, '[synthetic] ');
    const pending = first.receive({id: 'old-request', op: 'history', text: ''});
    first.disconnect();
    assert.equal(app.updates[1][1].titlePreface, 'Work: ', 'Disconnect restores the window title');
    assert.equal(app.ports.length, 1, 'Retry waits for its timer');
    const retry = app.timers.find(t => t.delay === 5000);
    assert.ok(retry, 'Reconnect after five seconds');
    retry.callback();
    assert.equal(app.ports.length, 2);
    const second = app.ports[1];
    app.finishSearch();
    await pending;
    assert.equal(first.sent.at(-1).id, 'old-request');
    assert.deepEqual(second.sent, [], 'An old request must never reply on the replacement port');
    await second.receive({id: 'new-request', op: 'list'});
    assert.deepEqual(second.sent, [{id: 'new-request', windows: []}]);
    await second.receive({id: 'bad-request', op: 'history', text: 'x'.repeat(513)});
    assert.equal(second.sent.at(-1).id, 'bad-request');
    assert.match(second.sent.at(-1).error, /Invalid history query/);
    second.disconnect();
    app.timers.filter(t => t.delay === 5000).at(-1).callback();
    assert.equal(app.ports.length, 3, 'Reconnect again after a second host exit');
    const restarted = boot();
    assert.equal(restarted.ports.length, 1, 'A fresh browser background context connects automatically');
    console.log('PASS: startup, delayed reconnect, repeated disconnect, title restoration, port isolation');
})().catch(error => { console.error(error); process.exitCode = 1; });
