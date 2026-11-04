import {expect, test} from 'bun:test';
import {mkdtempSync,mkdirSync,readFileSync,writeFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {spawnSync} from 'node:child_process';
import {runInNewContext} from 'node:vm';
const workflow=runInNewContext(readFileSync(new URL('../qml/Workflow.js',import.meta.url),'utf8')+'\n({project,decorate})');
const step=(id:string,kind='task',items:any[]=[])=>({id,title:id,kind,due:'',done:false,items});
for(const sequential of [false,true])test(`optimistic flow matches authority, workflow=${sequential}`,()=>{
    const root=mkdtempSync(join(tmpdir(),'tasktjik-projection-'));
    mkdirSync(join(root,'config/tasktjik'),{recursive:true});
    writeFileSync(join(root,'config/tasktjik/tasktjik.conf'),'[tasktjik]\nauthority=""\ndirectory='+JSON.stringify(join(root,'tasks'))+'\n');
    const env={...process.env,XDG_CONFIG_HOME:join(root,'config'),XDG_STATE_HOME:join(root,'state'),XDG_DATA_HOME:join(root,'data')};
    const call=(value:any,command='request')=>{const r=spawnSync(resolve(import.meta.dir,'../target/release/tasktjik-api'),[command],{env,input:JSON.stringify(value),encoding:'utf8'});if(r.status!==0)throw Error(r.stderr);return JSON.parse(r.stdout);};
    const read=()=>call({action:'list'},'state');
    try {
        const flow={...step('choice','flow',[step('agree','task',[step('branch')]),step('exit','exit'),{...step('remind','reminder'),offset:'one week before'}]),due:'2099-11-20'};
        call({action:'create',fields:{title:'Synthetic projection',workflow:sequential,subtasks:[step('sent'),flow,step('received')]}});
        for(const id of ['sent','remind','agree','branch','received','sent','sent','agree','exit','exit','agree']) {
            const state=read(),task=state.tasks[0],row=task.workflowRows.find((r:any)=>r.id===id);
            const expected=workflow.project(task,id,row.done!==true,state.today);
            expect(expected).not.toBeNull();
            call({action:'subtask-toggle',id:task.id,subtask_id:id,revision:task.revision});
            const actual=read().tasks[0];
            for(const field of ['workflowRows','subtaskPips','subtaskProgress'])expect(JSON.parse(JSON.stringify(expected[field]))).toEqual(actual[field]);
        }
        if(sequential){const state=read(),task=state.tasks[0];call({action:'subtask-toggle',id:task.id,subtask_id:'sent',revision:task.revision});const blocked=read();expect(workflow.project(blocked.tasks[0],'received',false,blocked.today)).toBeNull();}
    } finally {rmSync(root,{recursive:true,force:true});}
});

test('independent contribution pathways join only after both resolve, matching authority',()=>{
    const root=mkdtempSync(join(tmpdir(),'tasktjik-pathways-'));
    mkdirSync(join(root,'config/tasktjik'),{recursive:true});
    writeFileSync(join(root,'config/tasktjik/tasktjik.conf'),'[tasktjik]\nauthority=""\ndirectory='+JSON.stringify(join(root,'tasks'))+'\n');
    const env={...process.env,XDG_CONFIG_HOME:join(root,'config'),XDG_STATE_HOME:join(root,'state'),XDG_DATA_HOME:join(root,'data')};
    const call=(value:any,command='request')=>{const r=spawnSync(resolve(import.meta.dir,'../target/release/tasktjik-api'),[command],{env,input:JSON.stringify(value),encoding:'utf8'});if(r.status!==0)throw Error(r.stderr);return JSON.parse(r.stdout);};
    const projection=runInNewContext(readFileSync(new URL('../qml/Workflow.js',import.meta.url),'utf8')+'\n({project,decorate,cancel,instantiate})');
    const read=()=>call({action:'list'},'state');
    const requires=(id:string,dependencies:string[],extra={})=>({...step(id),requires:dependencies,...extra});
    const subtasks=[step('sent'),step('agreement','flow',[step('agreed'),step('declined','exit')]),step('received','group',[step('book'),step('video')]),requires('book-review',['book'],{kind:'flow',items:[step('verified'),step('revision','task',[step('fix')])]}),requires('video-review',['video']),requires('book-compiled',['book-review']),requires('video-compiled',['video-review']),requires('thanks',['book-compiled','video-compiled'],{acceptCancelled:true})];
    try {
        call({action:'create',fields:{title:'Synthetic contributions',workflow:true,subtasks}});
        const state=(id:string)=>read().tasks[0].workflowRows.find((r:any)=>r.id===id).status;
        const change=(id:string,cancel=false)=>{
            const before=read(),task=before.tasks[0],row=task.workflowRows.find((r:any)=>r.id===id);
            const expected=cancel?projection.cancel(task,id,before.today):projection.project(task,id,!row.done,before.today);
            expect(expected).not.toBeNull();
            call({action:cancel?'subtask-cancel':'subtask-toggle',id:task.id,revision:task.revision,subtask_id:id});
            const actual=read().tasks[0];
            for(const field of ['workflowRows','subtaskPips','subtaskProgress'])expect(JSON.parse(JSON.stringify(expected[field]))).toEqual(actual[field]);
        };
        expect(state('book')).toBe('blocked');
        change('sent');change('agreed');change('book');
        expect(state('book-review')).toBe('ready');expect(state('video-review')).toBe('blocked');
        change('revision');expect(state('book-compiled')).toBe('blocked');change('fix');change('book-compiled');
        expect(state('thanks')).toBe('blocked');change('video',true);
        expect(state('video-review')).toBe('cancelled');expect(state('video-compiled')).toBe('cancelled');expect(state('thanks')).toBe('ready');
        change('thanks');change('video',true);expect(state('thanks')).toBe('blocked');
        change('video');change('video-review');change('video-compiled');expect(state('thanks')).toBe('done');
        change('book',true);change('video',true);expect(state('thanks')).toBe('done');
        change('book',true);change('video',true);change('agreed');change('declined');expect(state('thanks')).toBe('done');
        change('declined');expect(state('thanks')).toBe('blocked');
        const source=read().tasks[0];
        const template=call({action:'create',fields:{title:'Synthetic pathway template',kind:'template',workflow:true,subtasks:source.subtasks}});
        const copy=call({action:'create',template_id:template.id,fields:{title:'Instantiated pathways'}});
        expect(copy.subtasks[3].requires).toEqual([copy.subtasks[2].items[0].id]);
        expect(copy.subtasks[7].requires).toEqual([copy.subtasks[5].id,copy.subtasks[6].id]);
        const localCopy=projection.instantiate(source.subtasks);
        expect(localCopy[3].requires).toEqual([localCopy[2].items[0].id]);
        for(const invalid of [[{...step('a'),requires:['a']}],[{...step('a'),requires:['missing']}],[{...step('a'),requires:['b']},step('b')]])expect(()=>call({action:'create',fields:{title:'Invalid',subtasks:invalid}})).toThrow();
    } finally {rmSync(root,{recursive:true,force:true});}
});
