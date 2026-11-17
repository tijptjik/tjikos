'use strict';
const $ = selector => document.querySelector(selector);
const form = $('#capture'), fields = form.elements;
let windowId, capture, choices = [], workflowDraft = [], workflowSequential = false;
function node(tag, text) {
    const element = document.createElement(tag);
    if (text !== undefined) element.textContent = text;
    return element;
}
async function send(op, extra = {}) {
    const result = await browser.runtime.sendMessage({op, windowId, request_id: capture?.request_id, ...extra});
    if (result?.error) throw Object.assign(Error(result.error), {rejected: result.rejected === true});
    return result;
}
function readFields(validate = true) {
    const repeated = parse(fields.repeatText.value);
    if (validate && repeated.error) throw Error(repeated.error);
    return {
        kind: 'reading', title: fields.title.value.trim(), reference: fields.reference.value,
        group: fields.group.value.trim() || 'All', tags: fields.tags.value.split(',').map(s => s.trim()).filter(Boolean),
        due: fields.due.value, repeat: repeated.error ? {} : repeated.rule,
        project: fields.project.value, subtasks: workflowDraft, workflow: workflowSequential,
        plan: fields.plan.value, assignee: fields.assignee.value,
        acceptance: fields.acceptance.value.split('\n').map(s => s.trim()).filter(Boolean),
        onlyInProject: fields.onlyInProject.checked, notifyBeforeDue: fields.notifyBeforeDue.checked,
        notifyBeforeDueLeadValue: Number(fields.notifyBeforeDueLeadValue.value),
        notifyBeforeDueLeadUnit: fields.notifyBeforeDueLeadUnit.value,
        // Retain invalid human input in the local draft until it can be corrected.
        ...(validate ? {} : {_repeatText: fields.repeatText.value})
    };
}
function selectOptions(select, rows, fallback, current) {
    select.replaceChildren();
    for (const [value, title] of rows) { const option = node('option', title); option.value = value; select.append(option); }
    if (current && !rows.some(([value]) => value === current)) { const option = node('option', current); option.value = current; select.append(option); }
    select.value = current || fallback;
}
async function loadOptions() {
    try {
        const result = await send('options');
        choices = result.items;
        $('#domains').replaceChildren(...result.groups.filter(g => g !== 'All').map(g => { const option = node('option'); option.value = g; return option; }));
        selectOptions(fields.project, [['', 'No project'], ...choices.filter(t => t.kind === 'project').map(t => [t.id, t.title])], '', fields.project.value);
        selectOptions(fields.template, [['', 'No template'], ...choices.filter(t => t.kind === 'template').map(t => [t.id, t.title])], '', '');
        $('#retry-options').hidden = true;
    } catch (error) { $('#error').textContent = error.message; $('#retry-options').hidden = false; }
}
async function load() {
    const result = await send('get');
    if (!result.capture) return;
    if (capture?.request_id === result.capture.request_id && !result.capture.saved) return;
    capture = result.capture;
    form.hidden = capture.saved;
    if (capture.saved) { $('#status').textContent = 'Saved to Reading. Your tab is still open.'; return; }
    const value = capture.submitted || capture.fields;
    for (const key of ['title', 'reference', 'due', 'plan', 'assignee']) fields[key].value = value[key] || '';
    fields.group.value = value.group === 'All' ? '' : value.group || ''; 
    selectOptions(fields.project, [['', 'No project']], '', value.project);
    fields.tags.value = (value.tags || []).join(', ');
    fields.acceptance.value = (value.acceptance || []).join('\n');
    fields.repeatText.value = value._repeatText ?? (value.repeat?.unit ? format(value.repeat) : '');
    fields.onlyInProject.checked = value.onlyInProject !== false;
    fields.notifyBeforeDue.checked = value.notifyBeforeDue !== false;
    fields.notifyBeforeDueLeadValue.value = value.notifyBeforeDueLeadValue ?? 1;
    fields.notifyBeforeDueLeadUnit.value = value.notifyBeforeDueLeadUnit || 'days';
    workflowDraft = structuredClone(value.subtasks || []); workflowSequential = value.workflow === true;
    renderWorkflowEditor();
    $('#fields').disabled = !!capture.submitted;
    $('#save').textContent = capture.submitted ? 'Retry original save' : 'Save to Reading';
    $('#status').textContent = capture.submitted ? 'The last save was not confirmed. Retry safely with the same capture.' : 'Save this page to your reading list.';
    $('#error').textContent = '';
    await loadOptions();
}
let draftWrites = Promise.resolve();
function remember() {
    if (!capture || capture.submitted || capture.saved) return;
    const value = readFields(false);
    draftWrites = draftWrites.then(() => send('draft', {fields: value})).catch(error => { $('#error').textContent = error.message; });
}
form.addEventListener('input', remember);
form.addEventListener('change', remember);
$('#workflow-editor').addEventListener('click', remember);
fields.repeatText.addEventListener('input', () => { const result = parse(fields.repeatText.value); $('#repeat-hint').textContent = result.error || result.label || ''; });
fields.template.addEventListener('change', () => {
    const template = choices.find(t => t.id === fields.template.value);
    if (!template) return;
    workflowDraft = taskWorkflow.instantiate(template.subtasks || []);
    workflowSequential = template.workflow === true;
    renderWorkflowEditor(); remember();
});
form.addEventListener('submit', async event => {
    event.preventDefault();
    $('#save').disabled = true;
    try {
        const value = capture.submitted || readFields();
        if (!['http:', 'https:'].includes(new URL(value.reference).protocol)) throw Error('Use an HTTP or HTTPS URL');
        await draftWrites;
        capture.submitted = structuredClone(value);
        $('#fields').disabled = true;
        await send('save', {fields: value});
        capture.saved = true;
        form.hidden = true;
        $('#status').textContent = 'Saved to Reading. Your tab is still open.';
    } catch (error) {
        $('#error').textContent = error.message;
        if (error.rejected) { capture.submitted = null; $('#fields').disabled = false; $('#save').textContent = 'Save to Reading'; }
        if (capture.submitted) $('#save').textContent = 'Retry original save';
    } finally { $('#save').disabled = false; }
});
$('#discard').addEventListener('click', async () => {
    if (capture.submitted && !confirm('The save may already have reached Tasktjik. Check Reading before saving this page again. Discard this local retry?')) return;
    try {
        await draftWrites;
        await send('discard'); capture = null; form.hidden = true;
        $('#status').textContent = 'Draft discarded. Use Ctrl+Shift+S to capture a page.';
    } catch (error) { $('#error').textContent = error.message; }
});
$('#retry-options').addEventListener('click', loadOptions);
browser.runtime.onMessage.addListener(message => {
    if (message.event === 'capture' && message.windowId === windowId) load().catch(error => { $('#error').textContent = error.message; });
});
browser.windows.getCurrent().then(window => { windowId = window.id; return load(); }).catch(error => { $('#status').textContent = error.message; });
