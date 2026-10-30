import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
const B=vm.createContext({});
vm.runInContext(readFileSync(new URL('../qml/BatteryLogic.js',import.meta.url),'utf8'),B);
vm.runInContext(readFileSync(new URL('../qml/MorphLogic.js',import.meta.url),'utf8'),B);
for(const [percent,count,tone] of [[0,9,'red'],[1,9,'red'],[2,9,'red'],[4.99,9,'red'],[5,1,'red'],[11,1,'red'],[12,2,'red'],[21,2,'red'],[22,3,'yellow'],[51,5,'yellow'],[52,6,'green'],[81,8,'green'],[82,9,'green'],[91,9,'green'],[100,9,'green']]) {
    assert.equal(B.pips(percent),count,`${percent}%`);
    assert.equal(B.tone(count,percent),tone);
}
for(const progress of [0,.25,.5,.75,1]) {
    const paths=B.interpolate(B.grid,B.flash,progress);
    assert.equal(paths.length,9);
    paths.forEach(path=>{
        assert.equal(path.points.length,48);
        assert.ok(path.points.flat().every(Number.isFinite));
    });
}
const grid=B.interpolate(B.grid,B.flash,0),flash=B.interpolate(B.grid,B.flash,1);
assert.ok(grid.every(p=>p.fill===1 && p.stroke===0 && p.opacity===1));
assert.equal(flash.filter(p=>p.opacity>0).length,1);
assert.equal(flash[4].fill,0);assert.equal(flash[4].stroke,1);
console.log('Battery thresholds, colour bands and morph geometry passed');
