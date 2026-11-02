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
