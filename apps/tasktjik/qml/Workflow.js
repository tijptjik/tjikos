// Client projection only. The authority validates every queued completion.
function reminderDate(due, offset) {
    const words=offset.toLowerCase().trim().split(/\s+/);
    let days=0;
    if(!['on deadline','on the deadline','same day'].includes(offset.toLowerCase().trim())) {
        const numbers={a:1,an:1,one:1,two:2,three:3,four:4,five:5,six:6,seven:7,eight:8,nine:9,ten:10};
        days=(numbers[words[0]]===undefined?Number(words[0]):numbers[words[0]])*(words[1].startsWith('week')?7:1);
    }
    const date=new Date(due+'T12:00:00Z');date.setUTCDate(date.getUTCDate()-days);
    return date.toISOString().slice(0,10);
}
function decorate(task,today) {
    const out=[],states=Object.create(null);
    function sequence(nodes,depth,cancelled,inheritedBlocked,sequential) {
        let stopped=cancelled,complete=true,previous='';
        for(const n of nodes || []) {
            const dependencies=n.requires;
            const dependencyCancelled=dependencies && dependencies.some(id=>states[id] && states[id].cancelled);
            const dependencyReady=dependencies && dependencies.every(id=>states[id] && (states[id].resolved || states[id].cancelled && n.acceptCancelled===true));
            const wasCancelled=(stopped && !(dependencyReady && n.acceptCancelled===true)) || n.cancelled===true || !!dependencyCancelled && n.acceptCancelled!==true;
            const blocked=!wasCancelled && (inheritedBlocked || (dependencies?!dependencyReady:sequential && !complete)),start=out.length,inheritedStop=stopped;
            stopped=wasCancelled;
            let resolved;
            if(n.kind==='flow') {
                const winner=n.items.find(c=>c.kind!=='reminder' && c.done);
                out.push(Object.assign({},n,{depth:depth,blocked:blocked&&!stopped,dependsOn:sequential&&!dependencies?previous:''}));
                resolved=!!winner;
                for(const c of n.items) {
                    const reminder=c.kind==='reminder',due=reminder?reminderDate(n.due,c.offset):n.due;
                    const cancelled=stopped || !!winner && winner.id!==c.id;
                    const status=reminder&&c.done?'done':stopped?'cancelled':blocked?'blocked':cancelled?'cancelled':c.done?'done':reminder&&due>today?'snoozed':'ready';
                    out.push(Object.assign({},c,{due:due,status:status,blocked:blocked&&!stopped,depth:depth+1,blockId:n.id}));
                    if(c.done&&!cancelled&&!reminder&&!blocked) {
                        const result=sequence(c.items,depth+1,false,false,false);
                        resolved=resolved&&result.complete;if(result.stopped)stopped=true;
                    }
                }
                out[start].status=wasCancelled?'cancelled':blocked?'blocked':resolved?'done':'ready';
                if(!blocked&&!stopped&&winner&&winner.kind==='exit')stopped=true;
            } else {
                out.push(Object.assign({},n,{status:stopped?'cancelled':blocked?'blocked':n.done?'done':'ready',depth:depth,blocked:blocked&&!stopped,dependsOn:sequential&&!dependencies?previous:''}));
                const result=sequence(n.items,depth+1,stopped,blocked,false);
                resolved=(n.done || n.kind==='group')&&result.complete;if(result.stopped)stopped=true;
                if(n.kind==='group'&&!stopped&&!blocked)out[start].status=resolved?'done':'ready';
            }
            states[n.id]={resolved:!!resolved&&!blocked&&!wasCancelled,cancelled:!!wasCancelled};
            if(wasCancelled&&!inheritedStop)stopped=false;
            if(!stopped)complete=complete&&(wasCancelled || resolved&&!blocked);
            previous=n.id;
        }
        return {stopped:stopped,complete:complete};
    }
    sequence(task.subtasks,0,false,false,task.workflow===true);
    const pips=out.filter(r=>!r.blockId).map(r=>({id:r.id,title:r.title,status:r.status,kind:r.kind}));
    const active=pips.filter(p=>p.status!=='cancelled');
    return Object.assign({},task,{workflowRows:out,subtaskPips:pips,subtaskProgress:{total:active.length,done:active.filter(p=>p.status==='done').length}});
}
function workflowNode(nodes,id) {for(const n of nodes || []){if(n.id===id)return n;const found=workflowNode(n.items,id);if(found)return found;}return null;}
function project(task,subtaskId,done,today) {
    const copy=JSON.parse(JSON.stringify(task));
    if(subtaskId) {
        const target=workflowNode(copy.subtasks,subtaskId);
        if(!target)return null;
        if(target.done===done)return decorate(copy,today);
        const row=decorate(copy,today).workflowRows.find(r=>r.id===subtaskId);
        if(!row || ['flow','group'].includes(row.kind) || row.blocked || ['cancelled','blocked'].includes(row.status))return null;
        target.done=done;
        return decorate(copy,today);
    }
    copy.done=done;copy.status=done?'done':'ready';
    copy.completions=copy.completions || {};
    if(done)copy.completions[copy.period || 'once']=new Date().toISOString();
    else delete copy.completions[copy.period || 'once'];
    copy.completedAt=Object.values(copy.completions).filter(Boolean).sort().slice(-1)[0] || null;
    return copy;
}

function cancel(task,id,today) {
    const copy=JSON.parse(JSON.stringify(task)),target=workflowNode(copy.subtasks,id);
    const row=decorate(copy,today).workflowRows.find(r=>r.id===id);
    if(!target || !row || row.blockId || !target.cancelled && (row.blocked || row.status==='cancelled'))return null;
    target.cancelled=!target.cancelled;
    return decorate(copy,today);
}
function instantiate(nodes) {
    const copy=JSON.parse(JSON.stringify(nodes)),ids=Object.create(null);
    function reset(list){for(const n of list){const id='xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g,c=>{const v=Math.floor(Math.random()*16);return (c==='x'?v:(v&3)|8).toString(16);});ids[n.id]=id;n.id=id;n.done=false;delete n.cancelled;reset(n.items || []);}}
    function remap(list){for(const n of list){if(n.requires)n.requires=n.requires.map(id=>ids[id]);remap(n.items || []);}}
    reset(copy);remap(copy);return copy;
}
var taskWorkflow={project:project,decorate:decorate,node:workflowNode,cancel:cancel,instantiate:instantiate};

function dependencyOptions(nodes,id) {
    const available=[];let found=false;
    function visit(list,choice){for(const n of list || []){if(n.id===id){found=true;return;}const start=available.length;visit(n.items,n.kind==='flow');if(found)return;if(n.kind==='flow'||choice)available.length=start;if(!choice)available.push({id:n.id,title:n.title});}}
    visit(nodes,false);return available;
}
