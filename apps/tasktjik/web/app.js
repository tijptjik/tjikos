'use strict';
const $=s=>document.querySelector(s), api='/tasktjik/api/v1/';
let items=[],groups=[],view='week',editing=null,snoozing=null,saveId=null,projectId='',collection='todo',editingKind='todo',workflowDraft=[],workflowSequential=false,showBlocked=false;
const expanded=new Set();
let completionQueue=[],completionSending=false,completionFailure='',mutationEpoch=0;
let selectedId='',projectReturnView='all',groupsRevision=0,settingsTab='groups',pendingGroup=null;
function toggleSubtasks(id){if(expanded.has(id))expanded.delete(id);else expanded.add(id);render();}
function newDefault(){editor(null,collection==='template'?'template':view==='projects'&&!projectId?'project':'todo');}
let deviceId=localStorage.getItem('tasktjik.device');if(!deviceId){deviceId=crypto.randomUUID();localStorage.setItem('tasktjik.device',deviceId);}
async function call(path,body){const r=await fetch(api+path,{method:body===undefined?'GET':'POST',headers:{'Content-Type':'application/json','X-Tasktjik-Request':'1','X-Tasktjik-Device':deviceId},body:body===undefined?undefined:JSON.stringify(body)});const value=await r.json();if(!r.ok)throw Error(value.error||`Request failed (${r.status})`);return value;}
async function mutate(body){return call('request',{request_id:crypto.randomUUID(),...body});}
function message(text){$('#message').textContent=text;}
function node(tag,text,cls){const n=document.createElement(tag);if(text!==undefined)n.textContent=text;if(cls)n.className=cls;return n;}
function localDate(d){return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,'0')}-${String(d.getDate()).padStart(2,'0')}`;}
async function refresh(){const epoch=mutationEpoch;const data=await call('tasks');if(epoch!==mutationEpoch)return;items=data.tasks;groups=data.groups;groupsRevision=data.groupsRevision||0;render();if($('#settings-dialog').open)renderSettings();}
function progress(t){const members=projectedItems().filter(x=>x.project===t.id&&x.kind==='todo');return {total:members.length,done:members.filter(x=>x.done||x.status==='cancelled').length};}
function openProject(t){projectReturnView=view;projectId=t.id;collection='todo';view='all';$('#search').value='';render();}
function due(t){const reminder=(t.workflowRows||[]).filter(r=>r.kind==='reminder'&&r.status==='ready').map(r=>r.due).sort()[0];return reminder&&(!t.due||reminder<t.due)?reminder:t.snoozedUntil?.slice(0,10)|| (t.period==='once'?'':t.period);}
function render(){
    const host=$('#tasks'),active=document.activeElement;
    const focus=active?.closest('article.task')?{task:active.closest('article.task').dataset.taskId,subtask:active.dataset.subtaskId}:null;
    const display=projectedItems();host.replaceChildren();$('#new').textContent=collection==='template'?'New template':view==='projects'&&!projectId?'New project':'New task';
    const project=items.find(t=>t.id===projectId),banner=$('#project-heading');banner.replaceChildren();
    if(project){const back=node('button',projectReturnView==='projects'?'‹ Projects':'‹ All tasks');back.onclick=()=>{projectId='';view=projectReturnView;render();};banner.append(back,node('h2',project.title));}
    else if(collection==='template')banner.append(node('h2','Templates'));
    const now=localDate(new Date()),end=new Date();if(view==='month')end.setMonth(end.getMonth()+1,0);else end.setDate(end.getDate()+6-(end.getDay()+6)%7);
    const bound=view==='today'?now:localDate(end),q=$('#search').value.toLowerCase();
    const rows=display.filter(t=>(collection==='todo'&&view==='projects'&&!projectId?t.kind==='project':(t.kind===collection||collection==='todo'&&t.kind==='project'&&!projectId))&&
        (projectId?t.project===projectId:!t.project||items.find(p=>p.id===t.project)?.onlyInProject!==true)&&
        t.status!=='cancelled'&&(showBlocked||t.status!=='blocked')&&`${t.title} ${t.group} ${t.tags.join(' ')}`.toLowerCase().includes(q)&&
        (collection==='template'||t.kind==='project'||view==='all'||!due(t)||due(t)&&due(t)<=bound||(t.completedAt&&localDate(new Date(t.completedAt))===now)));
    for(const group of groups){
        const tasks=rows.filter(t=>t.group===group);if(!tasks.length)continue;
        const section=node('section',undefined,'group');const heading=node('h2',`${group==='All'?'Inbox':group} · ${tasks.length}`);heading.ondblclick=()=>groupDialog('group-rename',group,'',tasks);section.append(heading);
        section.ondragover=e=>e.preventDefault();section.ondrop=e=>{e.preventDefault();const t=items.find(t=>t.id===e.dataTransfer.getData('text/plain'));if(t)run(section,async()=>{await mutate({action:'move',id:t.id,revision:t.revision,group});await refresh();});};
        for(const t of tasks){
            const card=node('article',undefined,'task'+(t.done?' done':'')),heading=node('div',undefined,'task-title');
            card.dataset.taskId=t.id;let clickTimer;card.tabIndex=0;card.onfocus=()=>selectedId=t.id;card.onclick=e=>{selectedId=t.id;if(e.target.closest('button,input'))return;clearTimeout(clickTimer);if(e.detail<2)clickTimer=setTimeout(()=>{if(t.kind==='project')openProject(t);else if(t.subtasks?.length)toggleSubtasks(t.id);},300);};card.ondblclick=e=>{clearTimeout(clickTimer);if(!e.target.closest('button,input'))editor(t);};card.draggable=!completionQueue.length;card.ondragstart=e=>e.dataTransfer.setData('text/plain',t.id);
            const toggle=node('button',t.kind==='project'?'▣':t.kind==='template'?'◇':t.done?'✓':'›');
            toggle.setAttribute('aria-label',`${t.kind==='project'?'Open':t.done?'Reopen':'Complete'} ${t.title}`);
            if(t.kind==='todo'){toggle.textContent='';toggle.className='complete-toggle';toggle.dataset.done=String(t.done);}
            toggle.onclick=e=>{e.stopPropagation();return t.kind==='project'?openProject(t):t.kind==='template'?editor(t):toggleCompletion(t.id,'');};
            const date=node('span',dateCode(due(t),now,view),'date-code');date.title=due(t)||'No due date';heading.append(date,toggle,node('h3',t.title));card.append(heading);
            if(t.kind==='project'){
                const p=progress(t),bar=node('progress');bar.max=p.total||1;bar.value=p.done;bar.setAttribute('aria-label','Project progress');
                const count=node('span',`${p.done}/${p.total}`,'project-count');const progressRow=node('div',undefined,'project-progress');progressRow.append(bar,count);card.append(progressRow);
            }
            const actions=node('div',undefined,'actions'),edit=node('button','✎');edit.setAttribute('aria-label','Edit '+t.title);edit.title='Edit (E)';edit.onclick=()=>editor(t);actions.append(edit);
            if(t.kind==='todo'){const snoozeButton=node('button','Snooze');snoozeButton.onclick=()=>{snoozing=t;$('#snooze .form-error').textContent='';$('#snooze').showModal();};actions.append(snoozeButton);}
            if(t.kind==='todo'||t.kind==='template'){
                if(t.subtasks?.length){const expand=node('button',undefined,'subtask-pips');expand.setAttribute('aria-label',`Subtasks: ${t.subtaskProgress?.done||0} of ${t.subtaskProgress?.total||0} complete`);expand.title=expand.getAttribute('aria-label');expand.setAttribute('aria-expanded',expanded.has(t.id));expand.onclick=()=>toggleSubtasks(t.id);for(const pip of t.subtaskPips||[])expand.append(node('span',undefined,'pip '+pip.status));card.append(expand);}
                else {const add=node('button','+','add-subtask');add.setAttribute('aria-label','Add subtask');add.onclick=()=>{editor(t);workflowDraft.push(newStep());renderWorkflowEditor();};actions.append(add);}
            }
            card.append(actions);
            if(expanded.has(t.id)){
                const list=node('div',undefined,'subtasks');
                for(const row of (t.workflowRows||[]).filter(row=>showBlocked||(!row.blocked&&row.status!=='blocked'))){
                    const line=node('button',undefined,'subtask '+row.status+' '+(row.kind==='task'?'':row.kind));line.disabled=!!row.blocked||row.kind==='flow'||['blocked','cancelled'].includes(row.status);line.setAttribute('aria-label',`${row.title}, ${row.status}${row.kind==='exit'?', exit option':row.kind==='reminder'?', reminder':''}`);line.title=(row.status==='snoozed'?'Snoozed until ':row.status==='blocked'?'Waiting for the preceding step · ':'')+(row.due||'');
                    const date=node('span',dateCode(row.due,now,view),'date-code'),dot=node('span',row.kind==='flow'?'⑂':'','subtask-dot');dot.style.marginLeft=`${row.depth*12}px`;
                    line.append(date,dot,node('span',row.title,'subtask-title'));
                    line.dataset.subtaskId=row.id;line.onfocus=()=>selectedId=t.id;line.onclick=()=>toggleCompletion(t.id,row.id);list.append(line);
                }
                const add=node('button','+');add.setAttribute('aria-label','Add subtask at end of list');add.onclick=()=>{editor(t);workflowDraft.push(newStep());renderWorkflowEditor();};list.append(add);card.append(list);
            }
            section.append(card);
        }
        host.append(section);
    }
    // Empty groups remain explicit drop destinations during a drag.
    host.ondragstart=()=>{for(const group of groups.filter(g=>!rows.some(t=>t.group===g))){const zone=node('section',group,'group drop-zone');zone.ondragover=e=>e.preventDefault();zone.ondrop=e=>{e.preventDefault();const t=items.find(t=>t.id===e.dataTransfer.getData('text/plain'));if(t)run(zone,async()=>{await mutate({action:'move',id:t.id,revision:t.revision,group});await refresh();});};host.append(zone);}};
    host.ondragend=()=>host.querySelectorAll('.drop-zone').forEach(n=>n.remove());
    if(!rows.length)host.append(node('p','No tasks in this view.','empty'));
    if(focus){const card=host.querySelector('[data-task-id="'+CSS.escape(focus.task)+'"]');const target=focus.subtask?card?.querySelector('[data-subtask-id="'+CSS.escape(focus.subtask)+'"]'):card;(target||card)?.focus({preventScroll:true});}
    if(completionFailure)message(completionFailure);
}
function newStep(kind='task'){return {id:crypto.randomUUID(),title:'',kind,due:'',done:false,items:[],offset:'one week before'};}
function renderWorkflowEditor(){
    const host=$('#workflow-editor');host.replaceChildren();host.hidden=!['todo','template'].includes(editingKind);if(host.hidden)return;
    host.append(node('h3','Subtasks & flows'));const workflow=node('button','Workflow');workflow.type='button';workflow.setAttribute('aria-pressed',workflowSequential);workflow.title='Each primary step waits for the preceding step';workflow.onclick=()=>{workflowSequential=!workflowSequential;renderWorkflowEditor();};host.append(workflow);
    function list(nodes,parent,container){
        nodes.forEach((step,i)=>{
            const box=node('fieldset'),title=node('input');title.value=step.title;title.placeholder=step.kind==='flow'?'Flow title':'Subtask title';title.setAttribute('aria-label','Subtask title');title.oninput=()=>step.title=title.value;box.append(title);
            if(parent?.kind==='flow'){
                const type=node('select');for(const [value,label] of [['task','Alternative'],['exit','Exit'],['reminder','Reminder']]){const option=node('option',label);option.value=value;type.append(option);}type.value=step.kind;type.setAttribute('aria-label','Alternative type');type.onchange=()=>{step.kind=type.value;renderWorkflowEditor();};box.append(type);
            }else{const date=node('input');date.type='date';date.value=step.due;date.setAttribute('aria-label','Step or flow deadline');date.oninput=()=>step.due=date.value;box.append(date);}
            if(step.kind==='reminder'){const offset=node('input');offset.value=step.offset||'one week before';offset.setAttribute('aria-label','Reminder relative to flow deadline');offset.oninput=()=>step.offset=offset.value;box.append(offset);}
            const remove=node('button','Remove');remove.type='button';remove.onclick=()=>{nodes.splice(i,1);renderWorkflowEditor();};box.append(remove);
            list(step.items,step,box);
            if(step.kind==='flow'||parent?.kind==='flow'&&step.kind==='task'){const add=node('button',step.kind==='flow'?'+ Alternative':'+ Branch subtask');add.type='button';add.onclick=()=>{step.items.push(newStep());renderWorkflowEditor();};box.append(add);}
            container.append(box);
        });
    }
    list(workflowDraft,null,host);
    for(const [kind,label] of [['task','+ Subtask'],['flow','+ Flow']]){const add=node('button',label);add.type='button';add.onclick=()=>{const step=newStep(kind);if(kind==='flow')step.items.push(newStep());workflowDraft.push(step);renderWorkflowEditor();};host.append(add);}
}
async function run(button,fn){button.disabled=true;try{message('');await fn();}catch(e){message(e.message);}finally{button.disabled=false;}}
function renderMetadata(task){
    const footer=$('#task-metadata'),metadata=task?.metadata||{};
    footer.textContent=Object.keys(metadata).sort().map(key=>key+' : '+JSON.stringify(metadata[key])).join('\n');
    footer.hidden=!task?.id||!footer.textContent;
}
function editor(t=null,kind='todo'){if(completionQueue.length)return;editing=t;editingKind=t?.kind||kind;workflowDraft=structuredClone(t?.subtasks||[]);workflowSequential=t?.workflow===true;saveId=crypto.randomUUID();const f=$('#task-form');f.reset();renderMetadata(t);$('#editor-title').textContent=(t?'Edit ':'New ')+(editingKind==='todo'?'task':editingKind);renderWorkflowEditor();const template=f.elements.template;template.replaceChildren(node('option','From Template…'));template.options[0].value='';for(const t of items.filter(t=>t.kind==='template')){const o=node('option',t.title);o.value=t.id;template.append(o);}template.onchange=()=>{const source=items.find(t=>t.id===template.value);if(source){workflowDraft=structuredClone(source.subtasks||[]);workflowSequential=source.workflow===true;function reset(nodes){for(const n of nodes){n.id=crypto.randomUUID();n.done=false;reset(n.items);}}reset(workflowDraft);renderWorkflowEditor();}};$('#template-choice').hidden=!!t||editingKind!=='todo';const project=f.elements.project;project.replaceChildren(node('option','No project'));project.options[0].value='';for(const p of items.filter(t=>t.kind==='project')){const o=node('option',p.title);o.value=p.id;project.append(o);}if(t?.project&&!items.some(p=>p.id===t.project&&p.kind==='project')){const o=node('option',t.project+' (legacy)');o.value=t.project;project.append(o);}project.value=t?.project||projectId;$('#project-choice').hidden=editingKind!=='todo';$('#project-visibility').hidden=editingKind!=='project';f.elements.onlyInProject.checked=t?.onlyInProject!==false;f.elements.title.value=t?.title||'';f.elements.group.value=t?.group||items.find(p=>p.id===projectId)?.group||'General';f.elements.due.value=t?.due||'';f.elements.tags.value=t?.tags.join(', ')||'';f.elements.repeatText.value=t?.repeat.unit?format(t.repeat):'';repeatSummary(!!t?.repeat.unit);f.elements.reference.value=t?.reference||'';f.elements.notify.checked=t?.notifyBeforeDue??true;f.elements.lead.value=t?.notifyBeforeDueLeadValue??1;f.elements.leadUnit.value=t?.notifyBeforeDueLeadUnit||'days';$('#group-options').replaceChildren(...groups.map(g=>{const o=node('option');o.value=g;return o;}));$('#editor .form-error').textContent='';$('#cancel-task').hidden=!t||editingKind!=='todo';$('#cancel-task').textContent=t?.status==='cancelled'?'Restore task (C)':'Cancel task (C)';$('#editor').showModal();f.elements.title.focus();}
function repeatSummary(commit){const input=$('#repeat-input'),value=parse(input.value);$('#repeat-hint').textContent=value.error||(!commit?value.label:'');const summarized=commit&&!value.error;input.hidden=summarized;$('#repeat-summary').hidden=!summarized;$('#repeat-summary').textContent=value.label;$('#repeat-edit').hidden=!summarized;}
$('#repeat-input').oninput=()=>repeatSummary(false);
$('#repeat-input').onblur=()=>repeatSummary(true);
$('#repeat-input').onkeydown=e=>{if(e.key==='Enter'){e.preventDefault();repeatSummary(true);if(!$('#repeat-edit').hidden)$('#repeat-edit').focus();}};
$('#repeat-edit').onclick=()=>{repeatSummary(false);$('#repeat-input').focus();$('#repeat-input').select();};
$('#task-form').onsubmit=async e=>{e.preventDefault();const f=e.target,b=f.querySelector('[type=submit]');b.disabled=true;try{const repeated=parse(f.elements.repeatText.value);if(repeated.error)throw Error(repeated.error);const fields={kind:editingKind,project:editingKind==='todo'?f.elements.project.value:'',subtasks:workflowDraft,workflow:workflowSequential,onlyInProject:f.elements.onlyInProject.checked,title:f.elements.title.value,group:f.elements.group.value,due:f.elements.due.value,tags:f.elements.tags.value.split(',').map(s=>s.trim()).filter(Boolean),repeat:{...repeated.rule,...(repeated.rule.unit&&editing?.repeat.start?{start:editing.repeat.start}:{})},reference:f.elements.reference.value,notifyBeforeDue:f.elements.notify.checked,notifyBeforeDueLeadValue:Number(f.elements.lead.value),notifyBeforeDueLeadUnit:f.elements.leadUnit.value};await mutate({request_id:saveId,action:editing?'edit':'create',...(editing?{id:editing.id,revision:editing.revision}:{}),fields});$('#editor').close();if(editingKind==='template'){settingsTab='templates';renderSettings();$('#settings-dialog').showModal();}await refresh();}catch(e){$('#editor .form-error').textContent=e.message;}finally{b.disabled=false;}};
async function snooze(until){await mutate({action:'snooze',id:snoozing.id,period:snoozing.period,until:until.toISOString()});$('#snooze').close();await refresh();}
for(const label of ['Afternoon','Evening','Night','Tomorrow','Weekend','Next Week']){const b=node('button',label);b.onclick=()=>run(b,async()=>{const d=new Date(),hours={Afternoon:14,Evening:18,Night:21};if(hours[label]){d.setHours(hours[label],0,0,0);if(d<=new Date())d.setDate(d.getDate()+1);}else{d.setHours(9,0,0,0);d.setDate(d.getDate()+(label==='Tomorrow'?1:label==='Weekend'?((6-d.getDay()+7)%7||7):((8-d.getDay())%7||7)));}await snooze(d);});$('#presets').append(b);}
$('#snooze-form').onsubmit=async e=>{e.preventDefault();try{await snooze(new Date(e.target.elements.until.value));}catch(e){$('#snooze .form-error').textContent=e.message;}};
$('#new').onclick=newDefault;$('#search').oninput=render;document.querySelectorAll('[data-view]').forEach(b=>b.onclick=()=>{view=b.dataset.view;collection='todo';if(view==='projects')projectId='';document.querySelectorAll('[data-view]').forEach(x=>x.classList.toggle('active',x===b));render();});document.querySelectorAll('[data-close]').forEach(b=>b.onclick=()=>document.getElementById(b.dataset.close).close());
$('#devices').onclick=()=>run($('#devices'),async()=>{const {devices}=await call('devices');const host=$('#device-list');host.replaceChildren();for(const d of devices){const l=node('label'),c=node('input');c.type='checkbox';c.checked=d.notifications;c.disabled=d.id===deviceId;c.onchange=async()=>{try{await call('devices',{id:d.id,name:d.name,notifications:c.checked});}catch(e){c.checked=!c.checked;message(e.message);}};l.append(c,node('span',d.name+(d.id===deviceId?' · this browser':'')));host.append(l);}$('#device-dialog').showModal();});
document.addEventListener('keydown',e=>{if(['INPUT','SELECT','TEXTAREA'].includes(e.target.tagName))return;if($('#editor').open && e.key.toLowerCase()==='c'){e.preventDefault();cancelTask();return;}if(document.querySelector('dialog[open]'))return;if(['ArrowDown','ArrowUp'].includes(e.key)){const rows=[...document.querySelectorAll('#tasks article.task,#tasks .subtask:not(:disabled)')],active=document.activeElement;let index=rows.indexOf(active);if(index<0)index=rows.indexOf(active?.closest('article.task'));if(index<0)index=rows.findIndex(r=>r.dataset.taskId===selectedId);if(rows.length){e.preventDefault();const next=rows[(index+(e.key==='ArrowDown'?1:-1)+rows.length)%rows.length];next.focus();next.scrollIntoView({block:'nearest'});}return;}if(e.key==='Enter'&&e.target.tagName!=='BUTTON'){const task=items.find(t=>t.id===selectedId);if(task?.kind==='project'){e.preventDefault();openProject(task);}else if(task?.subtasks?.length){e.preventDefault();toggleSubtasks(task.id);}}if(e.key==='n'){e.preventDefault();newDefault();}if(e.key==='e'){const task=items.find(t=>t.id===selectedId);if(task){e.preventDefault();editor(task);}}if(e.key==='d'){const task=items.find(t=>t.id===selectedId&&t.kind==='todo');if(task){e.preventDefault();toggleCompletion(task.id,'');}}if(e.key==='/'){e.preventDefault();$('#search').focus();}});
(async()=>{try{const identity=await call('identity');$('#identity').textContent=identity.login;const {devices}=await call('devices');if(!devices.some(d=>d.id===deviceId))await call('devices',{id:deviceId,name:'Browser',notifications:false});await refresh();}catch(e){message(e.message);}})();
setInterval(()=>{if(!document.hidden&&!document.querySelector('dialog[open]'))refresh().catch(e=>message(e.message));},30000);

async function cancelTask(){if(!editing||editing.kind!=='todo')return;await run($('#cancel-task'),async()=>{await mutate({action:'status',id:editing.id,revision:editing.revision,status:editing.status==='cancelled'?'ready':'cancelled',actor:'user',period:editing.period});$('#editor').close();await refresh();});}
$('#cancel-task').onclick=cancelTask;
function groupDialog(action,source='',target='',members=null){
    pendingGroup={action,source,target,groupsRevision,scoped:members!==null,members:(members||items.filter(t=>t.group===source)).map(t=>({id:t.id,revision:t.revision}))};
    const naming=['group-add','group-rename'].includes(action);$('#group-title').textContent=action==='group-add'?'New group':action==='group-rename'?'Rename group':'Confirm group change';
    $('#group-label').hidden=!naming;$('#group-name').required=naming;$('#group-name').value=source==='All'?'':source;
    $('#group-description').textContent=members?`Only these ${members.length} visible tasks will use the new name.`:action==='group-merge'?`Move all ${pendingGroup.members.length} tasks from “${source}” into “${target}” and remove “${source}”?`:action==='group-remove'?`Remove “${source}” and move its ${pendingGroup.members.length} tasks to Inbox?`:'';
    $('#group-dialog .form-error').textContent='';$('#group-dialog').showModal();if(naming){$('#group-name').focus();$('#group-name').select();}
}
$('#group-form').onsubmit=async e=>{e.preventDefault();const button=e.target.querySelector('[type=submit]');button.disabled=true;try{await mutate({...pendingGroup,target:['group-add','group-rename'].includes(pendingGroup.action)?$('#group-name').value.trim():pendingGroup.target});$('#group-dialog').close();await refresh();}catch(error){$('#group-dialog .form-error').textContent=error.message;}finally{button.disabled=false;}};
function renderSettings(){
    const host=$('#settings-list');host.replaceChildren();$('#settings-new').textContent=settingsTab==='groups'?'+ Group':'+ Template';$('#settings-hint').textContent=settingsTab==='groups'?'Drag a group into another to merge. Tasks keep their dates.':'Select a template to edit its subtasks and flows.';
    if(settingsTab==='templates'){for(const t of items.filter(t=>t.kind==='template')){const b=node('button',t.title);b.onclick=()=>{$('#settings-dialog').close();editor(t);};host.append(b);}return;}
    for(const group of groups){const row=node('div',undefined,'settings-group');row.draggable=group!=='All';row.ondragstart=e=>e.dataTransfer.setData('application/tasktjik-group',group);row.ondragover=e=>e.preventDefault();row.ondrop=e=>{e.preventDefault();const source=e.dataTransfer.getData('application/tasktjik-group');if(source&&source!==group&&source!=='All')groupDialog('group-merge',source,group);};const name=node('span',group==='All'?'Inbox':group);name.ondblclick=()=>{if(group!=='All')groupDialog('group-rename',group);};row.append(name);
        if(group!=='All'){const rename=node('button','✎'),remove=node('button','×');rename.setAttribute('aria-label','Rename '+group);remove.setAttribute('aria-label','Remove '+group);rename.onclick=()=>groupDialog('group-rename',group);remove.onclick=()=>groupDialog('group-remove',group,'All');row.append(rename,remove);}host.append(row);
    }
}
$('#settings').onclick=()=>{renderSettings();$('#settings-dialog').showModal();};
$('#settings-groups').onclick=()=>{settingsTab='groups';renderSettings();};$('#settings-templates').onclick=()=>{settingsTab='templates';renderSettings();};
$('#settings-new').onclick=()=>{if(settingsTab==='groups')groupDialog('group-add');else{$('#settings-dialog').close();editor(null,'template');}};

$('#show-blocked').onclick=()=>{showBlocked=!showBlocked;$('#show-blocked').setAttribute('aria-pressed',showBlocked);render();};

function projectedItems(){let projected=items;for(const operation of completionQueue)projected=projected.map(t=>t.id===operation.id?(taskWorkflow.project(t,operation.subtaskId,operation.done,localDate(new Date()))||t):t);return projected;}
function toggleCompletion(id,subtaskId){const task=projectedItems().find(t=>t.id===id),target=task&&(subtaskId?taskWorkflow.node(task.subtasks,subtaskId):task);if(!target)return;const operation={id,subtaskId,done:!target.done,requestId:crypto.randomUUID()};if(!taskWorkflow.project(task,subtaskId,operation.done,localDate(new Date())))return;completionFailure='';message('');completionQueue.push(operation);render();sendCompletion();}
async function sendCompletion(){
    if(completionSending||!completionQueue.length)return;
    completionSending=true;const operation=completionQueue[0],task=items.find(t=>t.id===operation.id),target=task&&(operation.subtaskId?taskWorkflow.node(task.subtasks,operation.subtaskId):task);
    try {
        if(!target||!taskWorkflow.project(task,operation.subtaskId,operation.done,localDate(new Date())))throw Error('This step changed; review its current state.');
        if(target.done!==operation.done){mutationEpoch++;const reply=(await mutate({action:operation.subtaskId?'subtask-toggle':'toggle',id:task.id,subtask_id:operation.subtaskId,revision:task.revision,period:task.period,request_id:operation.requestId})).result;
            const saved=taskWorkflow.decorate({...reply,period:task.period,done:!!reply.completions?.[task.period||'once']},localDate(new Date()));items=items.map(t=>t.id===saved.id?saved:t);mutationEpoch++;}
        completionQueue.shift();
    } catch(error) {completionQueue=completionQueue.filter(op=>op.id!==operation.id);completionFailure=error.message;}
    finally {completionSending=false;render();sendCompletion();}
}
const taskTip=node('div',undefined,'task-tooltip');taskTip.role='tooltip';taskTip.hidden=true;document.body.append(taskTip);let tipTimer;
function hideTaskTip(){clearTimeout(tipTimer);taskTip.hidden=true;}
function showTaskTip(event){const owner=event.target.closest('[title],[data-tooltip]');hideTaskTip();if(!owner)return;if(owner.hasAttribute('title')){owner.dataset.tooltip=owner.title;owner.removeAttribute('title');}if(!owner.dataset.tooltip)return;tipTimer=setTimeout(()=>{if(!owner.isConnected)return;(owner.closest('dialog')||document.body).append(taskTip);taskTip.textContent=owner.dataset.tooltip;taskTip.hidden=false;const rect=owner.getBoundingClientRect();taskTip.style.left=Math.max(8,Math.min(innerWidth-taskTip.offsetWidth-8,rect.left))+'px';taskTip.style.top=(rect.bottom+taskTip.offsetHeight+8<innerHeight?rect.bottom+6:Math.max(8,rect.top-taskTip.offsetHeight-6))+'px';},550);}
document.addEventListener('pointerover',showTaskTip);document.addEventListener('focusin',showTaskTip);document.addEventListener('pointerout',hideTaskTip);document.addEventListener('focusout',hideTaskTip);document.addEventListener('pointerdown',hideTaskTip);
