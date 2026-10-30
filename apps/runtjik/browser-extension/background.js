/* No URLs, page content, or titles are persisted by this extension. */
let port;
const prefixes = new Map();

async function release(windowId, expected) {
    if (!prefixes.has(windowId)) return;
    const record = prefixes.get(windowId);
    if (expected && expected !== record) return;
    const prefix = record.prefix;
    prefixes.delete(windowId);
    await browser.windows.update(windowId, {titlePreface: prefix});
}

async function handle(message) {
    const windowId = message.window;
    if (message.op === 'list') {
        const windows = await browser.windows.getAll({populate: true, windowTypes: ['normal']});
        return {windows: windows.filter(w => !w.incognito).map(w => ({
            id: w.id,
            title: (w.tabs.find(t => t.active) || {}).title || 'Browser window',
            favicon: (w.tabs.find(t => t.active) || {}).favIconUrl || '',
            tabs: w.tabs.filter(t => !t.pinned).map(t => ({id: t.id, title: t.title || 'Untitled',
                url: t.url || '', favicon: t.favIconUrl || '', pinned: !!t.pinned,
                audible: !!t.audible, muted: !!t.mutedInfo?.muted, discarded: !!t.discarded}))
        }))};
    }
    if (message.op === 'release') {
        await release(windowId);
        return {};
    }
    if (!['focus', 'details', 'close'].includes(message.op) || !Number.isInteger(windowId)) throw new Error('Invalid focus request');
    const window = await browser.windows.get(windowId);
    if (window.incognito) throw new Error('Private windows are excluded');
    if (message.op === 'close' && (message.tab === null || message.tab === undefined)) {
        await browser.windows.remove(windowId);
        return {};
    }
    if (message.tab !== null && message.tab !== undefined) {
        const tab = await browser.tabs.get(message.tab);
        if (tab.windowId !== windowId) throw new Error('Tab moved; refresh the list');
        if (tab.pinned) throw new Error('Pinned tabs are excluded');
        if (message.op === 'details') return {tab: {id: tab.id, title: tab.title || 'Untitled', url: tab.url || ''}};
        if (message.op === 'close') {
            if (tab.url !== message.url) throw new Error('Tab navigated; it was left open');
            await browser.tabs.remove(tab.id);
            return {};
        }
        await browser.tabs.update(tab.id, {active: true});
    }
    if (message.op !== 'focus') throw new Error('Select a tab');
    const record = {prefix: window.titlePreface || ''};
    prefixes.set(windowId, record);
    await browser.windows.update(windowId, {titlePreface: message.marker, focused: true});
    // Also restore the title if the host disappears during focus.
    setTimeout(() => release(windowId, record).catch(() => {}), 10000);
    return {};
}

function connect() {
    port = browser.runtime.connectNative('org.tjikos.runtjik_browser');
    const connection = port;
    connection.onMessage.addListener(async message => {
        try {
            connection.postMessage({id: message.id, ...await handle(message)});
        } catch (error) {
            connection.postMessage({id: message.id, error: error.message});
        }
    });
    connection.onDisconnect.addListener(() => {
        for (const id of prefixes.keys()) release(id).catch(() => {});
        setTimeout(connect, 5000);
    });
}
connect();
