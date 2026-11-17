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
    if (message.op === 'history') {
        if (typeof message.text !== 'string' || message.text.length > 512) throw new Error('Invalid history query');
        if (!browser.history) throw new Error('Reload the updated bridge and allow history access');
        const items = await browser.history.search({text: message.text, startTime: 0, maxResults: 100});
        return {history: items.filter(item => {
            try { return ['http:', 'https:'].includes(new URL(item.url).protocol); } catch (_) { return false; }
        }).map(item => ({id: item.id, title: (item.title || item.url).slice(0, 4096),
            url: item.url, lastVisitTime: item.lastVisitTime || 0, visitCount: item.visitCount || 0}))};
    }
    if (message.op === 'delete-history') {
        const url = new URL(message.url);
        if (!['http:', 'https:'].includes(url.protocol)) throw new Error('Only HTTP(S) history entries can be deleted');
        await browser.history.deleteUrl({url: message.url});
        return {deleted: true};
    }
    if (message.op === 'open-history') {
        const url = new URL(message.url);
        if (!['http:', 'https:'].includes(url.protocol)) throw new Error('Only HTTP(S) history entries can be opened');
        if (!(await browser.history.getVisits({url: message.url})).length) throw new Error('History entry was removed; refresh the list');
        const windows = (await browser.windows.getAll({windowTypes: ['normal']})).filter(w => !w.incognito);
        const target = windows.find(w => w.focused) || windows[0];
        let tab;
        if (target) tab = await browser.tabs.create({windowId: target.id, url: message.url, active: true});
        else {
            const created = await browser.windows.create({url: message.url, incognito: false});
            tab = created.tabs[0];
        }
        await handle({op: 'focus', window: tab.windowId, tab: tab.id, marker: message.marker});
        return {window: tab.windowId, tab: tab.id};
    }
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
