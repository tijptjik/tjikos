import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';

const T=vm.createContext({});
vm.runInContext(readFileSync(new URL('../qml/CodexTimeline.js',import.meta.url),'utf8'),T);
const now=1000000;
const reset=now+(6*24+18)*3600;
const hours=T.countdown(reset,now).hours;
assert.equal(T.countdown(reset,now).label,'6d 18h');
assert.deepEqual(Array.from({length:7},(_,day)=>T.dayHours(hours,day)),[24,24,24,24,24,24,18]);
assert.equal(T.countdown(reset,now+3600).label,'6d 17h');
assert.equal(T.dayHours(T.countdown(now+1800,now).hours,0),.5);
assert.equal(T.countdown(now+1,now).label,'1m');
for(const end of [now,now-1000]) {
    assert.equal(T.countdown(end,now).hours,0);
    assert.equal(T.countdown(end,now).label,'Due now');
}
for(const end of [null,undefined,NaN,'123']) {
    assert.equal(T.countdown(end,now).known,false);
    assert.equal(T.countdown(end,now).label,'Unknown');
}
assert.equal(T.countdown(now+10*86400,now).hours,168);
for(const hour of [0,1,23,24,25,48,84,162,168]) {
    const days=Array.from({length:7},(_,day)=>T.dayHours(hour,day));
    assert.equal(days.reduce((a,b)=>a+b,0),hour);
    assert.ok(days.filter(value=>value>0 && value<24).length<=1);
}
const near=(actual,expected)=>assert.ok(Math.abs(actual-expected)<1e-9,`${actual} != ${expected}`);
for(const [hours,credit] of [[162,.92],[162,.98],[126,.75],[0,.5],[168,0],[168,1],[0,0],[.5,.001]]) {
    const slots=Array.from({length:7},(_,day)=>T.comparison(hours,credit,day));
    const total=key=>slots.reduce((sum,slot)=>sum+slot[key],0)/7;
    near(total('overlap'),Math.min(hours/168,credit));
    near(total('timeOnly'),Math.max(0,hours/168-credit));
    near(total('creditOnly'),Math.max(0,credit-hours/168));
    near(total('empty'),1-Math.max(hours/168,credit));
    for(const slot of slots) {
        assert.ok(slot.known);
        near(slot.overlap+slot.timeOnly+slot.creditOnly+slot.empty,1);
        assert.ok(slot.timeOnly===0 || slot.creditOnly===0);
    }
}
for(const [hours,credit] of [[null,.5],[100,-1],[100,undefined],[NaN,.5],[100,NaN]]) {
    assert.equal(T.comparison(hours,credit,0).known,false);
    assert.equal(T.comparison(hours,credit,0).overlap,0);
}
for(const [credit,label] of [[.96,'96%'],[.925,'92.5%'],[0,'0%'],[1,'100%'],[null,'—'],[-1,'—']])
    assert.equal(T.remainingLabel(credit),label);
console.log('Codex countdown: time boundaries, overlap/deficit/surplus, missing data and remaining percentages passed');
