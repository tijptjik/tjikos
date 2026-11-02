import {expect, test} from 'bun:test';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
const {dateCode} = runInNewContext(readFileSync(new URL('../qml/DateCode.js', import.meta.url), 'utf8')+'\n({dateCode})');
test('primary and subtask dates share compact codes across calendar views',()=>{
    expect(dateCode('', '2026-11-02', 'all')).toBe('·');
    expect(dateCode('2026-11-06', '2026-11-02', 'all')).toBe('FRI');
    expect(dateCode('2026-11-13', '2026-11-02', 'all')).toBe('1W');
    expect(dateCode('2026-11-20', '2026-11-02', 'month')).toBe('2W');
    expect(dateCode('2026-11-20', '2026-11-02', 'week')).toBe('FRI');
    expect(dateCode('2026-10-31', '2026-11-02', 'all')).toBe('2D');
    expect(dateCode('2027-02-02', '2026-11-02', 'all')).toBe('3M');
    expect(dateCode('2027-11-02', '2026-11-02', 'all')).toBe('1Y');
});
