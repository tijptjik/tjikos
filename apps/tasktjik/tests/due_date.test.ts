import {expect, test} from 'bun:test';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';

const {parse, suggestions} = runInNewContext(readFileSync(new URL('../qml/DueDate.js', import.meta.url), 'utf8') + '\n({parse, suggestions})');
const anchor = new Date(2026, 10, 1, 23, 30); // Sunday, local time
const cases = [
    ['', ''], ['  ', ''], ['no due date', ''], ['none', ''],
    ['2026-12-25', '2026-12-25'], [' Today ', '2026-11-01'],
    ['TOMORROW', '2026-11-02'], ['day after tomorrow', '2026-11-03'],
    ['yesterday', '2026-10-31'], ['Friday', '2026-11-06'],
    ['next Friday', '2026-11-06'], ['sun', '2026-11-01'],
    ['this Sunday', '2026-11-01'], ['next Sunday', '2026-11-08'],
    ['on Monday', '2026-11-02'], ['next week', '2026-11-08'],
    ['next month', '2026-12-01'], ['next year', '2027-11-01'],
    ['in 2 weeks', '2026-11-15'], ['in two weeks', '2026-11-15'],
    ['3 days from now', '2026-11-04'], ['a month', '2026-12-01'],
    ['two days ago', '2026-10-30'], ['in 0 days', '2026-11-01'],
    ['end of month', '2026-11-30'], ['15 December', '2026-12-15'],
    ['Dec 15th, 2027', '2027-12-15'], ['on 1st January', '2027-01-01'],
    ['Nov 1', '2026-11-01'], ['Feb 29', '2028-02-29'],
    ['29 Feb 2024', '2024-02-29'], ['in   three   days', '2026-11-04'],
];
for (const [input, expected] of cases) {
    test(input || 'empty date', () => expect(parse(input, anchor)).toEqual({value: expected, error: ''}));
}
for (const input of ['2026-02-29', '2026-04-31', '2026-13-01', '0000-01-01',
    'February 30', '31 April', '29 Feb 2025', '03/04/2026', 'tomorrow at 5pm',
    'eventually', 'next Frid', 'in -2 days', 'in 1.5 weeks', 'in 999999999999999 years']) {
    test('reject ' + input, () => {
        expect(parse(input, anchor).error).toContain('Date not understood');
        expect(parse(input, anchor).value).toBe('');
    });
}
test('calendar arithmetic clamps month and leap-year boundaries', () => {
    expect(parse('next month', new Date(2024, 0, 31)).value).toBe('2024-02-29');
    expect(parse('next year', new Date(2024, 1, 29)).value).toBe('2025-02-28');
    expect(parse('tomorrow', new Date(2026, 11, 31)).value).toBe('2027-01-01');
    expect(parse('in 2 days', new Date(2026, 2, 7)).value).toBe('2026-03-09');
    expect(parse('tomorrow', new Date(9999, 11, 31)).error).not.toBe('');
});

test('autocomplete offers resolved phrases without changing complete dates', () => {
    expect(suggestions('tom', anchor)[0]).toMatchObject({label: 'tomorrow', value: '2026-11-02'});
    expect(suggestions('next f', anchor)[0]).toMatchObject({label: 'next Friday', value: '2026-11-06'});
    expect(suggestions('in 3 w', anchor)[0].value).toBe('2026-11-22');
    expect(suggestions('in two w', anchor)[0].value).toBe('2026-11-15');
    expect(suggestions('', anchor)).toHaveLength(4);
    expect(suggestions('tomorrow', anchor)).toEqual([]);
    expect(suggestions('2026-11-02', anchor)).toEqual([]);
    expect(suggestions('gibberish', anchor)).toEqual([]);
});
