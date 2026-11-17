'use strict';
const captures = new Map();
const native = async message => {
    const result = await browser.runtime.sendNativeMessage('org.tjikos.readtjik', message);
    if (result.error) throw Object.assign(Error(result.error), {rejected: result.rejected === true});
    return result;
};
function validPage(tab) {
    if (!tab || tab.incognito) throw Error('Private windows are excluded');
    const url = new URL(tab.url);
    if (!['http:', 'https:'].includes(url.protocol)) throw Error('Open an HTTP or HTTPS page to save it');
    return {kind: 'reading', title: tab.title || tab.url, reference: tab.url, group: 'All', tags: [], due: '', repeat: {}};
}
async function persist(capture) {
    await browser.storage.local.set({[capture.key]: capture});
    captures.set(capture.key, capture);
}
const capturing = new Map();
async function captureTab(tab) {
    const key = tab?.windowId;
    if (capturing.has(key)) return capturing.get(key);
    const operation = resolveCapture(tab);
    capturing.set(key, operation);
    try { return await operation; } finally { capturing.delete(key); }
}
async function resolveCapture(tab) {
    const fields = validPage(tab), key = `capture:${tab.windowId}`;
    // Recover uncertain submissions across extension/browser restarts. Never silently
    // replace a pending capture with a different page.
    let previous = captures.get(key) || (await browser.storage.local.get(key))[key];
    if (!previous) {
        // Window IDs change across browser restarts. Recover a matching orphaned draft.
        const live = new Set((await browser.windows.getAll()).map(window => window.id));
        const stored = await browser.storage.local.get(null);
        previous = Object.values(stored).find(draft => draft?.key?.startsWith('capture:') &&
            !live.has(draft.windowId) && !draft.saved && draft.fields.reference === fields.reference);
        if (previous) {
            const oldKey = previous.key;
            previous.key = key; previous.windowId = tab.windowId;
            await persist(previous);
            await browser.storage.local.remove(oldKey);
        }
    }
    if (previous && (!previous.saved || previous.fields.reference === fields.reference)) return previous;
    const capture = {key, windowId: tab.windowId, fields, request_id: crypto.randomUUID(), submitted: null, saved: false};
    await persist(capture);
    return capture;
}
const saving = new Map();
async function save(capture, fields) {
    if (capture.saved) return {saved: true};
    if (saving.has(capture.key)) return saving.get(capture.key);
    if (capture.submitted && JSON.stringify(capture.submitted) !== JSON.stringify(fields)) {
        throw Error('This save has an uncertain outcome. Retry the original submission before changing it.');
    }
    const operation = (async () => {
        capture.fields = fields;
        capture.submitted = structuredClone(fields);
        await persist(capture); // Freeze UUID and exact fields before the request leaves.
        let result;
        try {
            result = await native({op: 'save', request_id: capture.request_id, fields: capture.submitted});
        } catch (error) {
            if (error.rejected) { capture.submitted = null; await persist(capture); }
            throw error;
        }
        if (!result.saved) throw Error('Save was not acknowledged');
        capture.saved = true;
        // Remove page data after acknowledgement, retain only an in-memory result.
        await browser.storage.local.remove(capture.key);
        captures.set(capture.key, capture);
        return result;
    })();
    saving.set(capture.key, operation);
    try { return await operation; } finally { saving.delete(capture.key); }
}
async function notify(message) {
    await browser.notifications.create({type: 'basic', iconUrl: browser.runtime.getURL('icon.svg'), title: 'Tasktjik', message});
}
async function currentTab() {
    return (await browser.tabs.query({active: true, currentWindow: true}))[0];
}
async function command(name, suppliedTab) {
    // Firefox requires sidebar.open directly in the user gesture, before awaiting.
    const opening = name === 'save-panel' ? browser.sidebarAction.open() : Promise.resolve();
    try {
        const tab = suppliedTab || await currentTab();
        const capture = await captureTab(tab);
        await opening;
        if (name === 'save-inbox') {
            if (!capture.saved && (capture.fields.reference !== tab.url || capture.submitted || capture.edited)) {
                throw Error('A reading draft is pending. Open Tasktjik to finish or discard it first.');
            }
            await save(capture, capture.fields);
            await notify('Saved to Reading → Inbox');
        }
        await browser.runtime.sendMessage({event: 'capture', windowId: tab.windowId}).catch(() => {});
    } catch (error) {
        await opening.catch(() => {});
        await notify(error.message);
    }
}
browser.commands.onCommand.addListener(name => command(name));
browser.browserAction.onClicked.addListener(tab => command('save-panel', tab));
browser.runtime.onMessage.addListener(async (message, sender) => {
    if (!sender.url || !sender.url.startsWith(browser.runtime.getURL('panel.html'))) return;
    const key = `capture:${message.windowId}`;
    const capture = captures.get(key) || (await browser.storage.local.get(key))[key];
    try {
        if (message.op === 'get') return {capture};
        if (message.op === 'options') return await native({op: 'options'});
        if (!capture || capture.request_id !== message.request_id) throw Error('This capture changed; reopen the panel');
        if (message.op === 'save') return await save(capture, message.fields);
        if (message.op === 'draft') {
            if (capture.submitted || capture.saved) return {};
            capture.fields = message.fields;
            capture.edited = true;
            await persist(capture);
            return {};
        }
        if (message.op === 'discard') {
            if (saving.has(key)) throw Error('Wait for the save to finish');
            captures.delete(key);
            await browser.storage.local.remove(key);
            return {};
        }
        throw Error('Unknown panel operation');
    } catch (error) { return {error: error.message, rejected: error.rejected === true}; }
});
