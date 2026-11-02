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
    const out=[];
    function sequence(nodes,depth,cancelled,inheritedBlocked,sequential) {
        let stopped=cancelled,complete=true,previous='';
        for(const n of nodes || []) {
            const blocked=inheritedBlocked || (sequential && !complete),wasCancelled=stopped,start=out.length;
            let resolved;
            if(n.kind==='flow') {
                const winner=n.items.find(c=>c.kind!=='reminder' && c.done);
                out.push(Object.assign({},n,{depth:depth,blocked:blocked&&!stopped,dependsOn:sequential?previous:''}));
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
                out.push(Object.assign({},n,{status:stopped?'cancelled':blocked?'blocked':n.done?'done':'ready',depth:depth,blocked:blocked&&!stopped,dependsOn:sequential?previous:''}));
                const result=sequence(n.items,depth+1,stopped,blocked,false);
                resolved=n.done&&result.complete;if(result.stopped)stopped=true;
            }
            if(!stopped)complete=complete&&resolved&&!blocked;
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
        if(!row || row.kind==='flow' || row.blocked || ['cancelled','blocked'].includes(row.status))return null;
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

var taskWorkflow={project:project,decorate:decorate,node:workflowNode};
