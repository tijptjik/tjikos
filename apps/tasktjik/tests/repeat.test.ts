import {expect, test} from 'bun:test';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
const {parse, format} = runInNewContext(readFileSync(new URL('../qml/Repeat.js', import.meta.url), 'utf8')+'\n({parse, format})');
const cases: [string, object][] = [
    ['', {}], ['none', {}], ['Does not repeat', {}],
    ['daily', {unit:'days',value:1}], ['every other day', {unit:'days',value:2}],
    ['every two weeks', {unit:'weeks',value:2}], ['fortnightly', {unit:'weeks',value:2}],
    ['monthly', {unit:'months',value:1}], ['every 3 months', {unit:'months',value:3}],
    ['annually', {unit:'years',value:1}], ['every weekday', {unit:'days-of-the-week',value:'1,2,3,4,5'}],
    ['every weekend', {unit:'days-of-the-week',value:'6,7'}],
    ['every Monday and Friday', {unit:'days-of-the-week',value:'1,5'}],
    ['on tue, thu', {unit:'days-of-the-week',value:'2,4'}],
    ['Sunday every week', {unit:'days-of-the-week',value:'7'}],
    ['on the 1st and 15th of every month', {unit:'days-of-the-month',value:'1,15'}],
    ['every month on the 31st', {unit:'days-of-the-month',value:'31'}],
    ['  every   2 weeks ', {unit:'weeks',value:2}],
];
for (const [text,rule] of cases)test(text||'no repeat',()=>{
    expect(parse(text).error).toBe('');expect(parse(text).rule).toEqual(rule);
    expect(parse(format(rule)).rule).toEqual(rule);
});
for(const text of ['every 0 days','every -1 weeks','every 10001 years','every 1.5 months','sometimes','every first Monday','32nd of every month','every Monday afternoon','every month on the 0th'])test('reject '+text,()=>expect(parse(text).error).not.toBe(''));
