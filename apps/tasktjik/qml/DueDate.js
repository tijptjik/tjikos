// Calendar dates only: never parse a time or rely on locale-dependent Date.parse.
function suggestions(input, now) {
    const query = input.trim().toLowerCase().replace(/\s+/g, ' ');
    // Complete unfinished phrases; a complete date keeps Enter's save behavior.
    if (query && !parse(query, now).error)return [];
    let choices = ['today', 'tomorrow', 'next week', 'in 2 weeks', 'next month',
        'day after tomorrow', 'end of month', 'no due date'];
    const weekdays = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];
    for (const prefix of ['', 'next ', 'this '])choices = choices.concat(weekdays.map(day => prefix + day));
    const count = /^(?:in )?(\d+|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve)(?: |$)/.exec(query);
    if (count) {
        const prefix = query.startsWith('in ') ? 'in ' : '';
        choices = choices.concat(['days', 'weeks', 'months', 'years'].map(unit => prefix + count[1] + ' ' + unit));
    }
    return choices.filter(label => label.toLowerCase().startsWith(query))
        .map(label => ({label: label, value: parse(label, now).value, error: parse(label, now).error}))
        .filter(item => !item.error).slice(0, 4);
}

function iso(date) {
    return String(date.getFullYear()).padStart(4, '0') + '-'
        + String(date.getMonth() + 1).padStart(2, '0') + '-'
        + String(date.getDate()).padStart(2, '0');
}

function calendar(year, month, day) {
    const date = new Date(2000, month, day, 12);
    date.setFullYear(year);
    return year >= 1 && year <= 9999 && date.getFullYear() === year
        && date.getMonth() === month && date.getDate() === day ? date : null;
}

function parse(input, now) {
    const text = input.trim().toLowerCase().replace(/\s+/g, ' ');
    const error = {value: '', error: 'Date not understood. Try “tomorrow”, “next Friday”, “in 2 weeks”, or YYYY-MM-DD.'};
    function result(date) {
        return date && Number.isFinite(date.getTime()) && date.getFullYear() >= 1
            && date.getFullYear() <= 9999 ? {value: iso(date), error: ''} : error;
    }
    if (!text || ['none', 'no date', 'no due date'].includes(text))return {value: '', error: ''};
    const today = calendar(now.getFullYear(), now.getMonth(), now.getDate());
    if (!today)return error;
    function days(count) {
        const date = new Date(today.getTime());
        date.setDate(date.getDate() + count);
        return result(date);
    }
    function months(count) {
        const date = new Date(today.getTime());
        date.setDate(1);
        date.setMonth(date.getMonth() + count);
        const last = new Date(date.getFullYear(), date.getMonth() + 1, 0, 12).getDate();
        date.setDate(Math.min(today.getDate(), last));
        return result(date);
    }
    let match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text);
    if (match)return result(calendar(Number(match[1]), Number(match[2]) - 1, Number(match[3])));
    const relative = {today: 0, tomorrow: 1, yesterday: -1, 'day after tomorrow': 2};
    if (Object.prototype.hasOwnProperty.call(relative, text))return days(relative[text]);

    const weekdays = ['sunday', 'monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday'];
    match = /^(?:(next|this|on) )?([a-z]+)$/.exec(text);
    if (match) {
        const weekday = weekdays.findIndex(day => day === match[2] || day.slice(0, 3) === match[2]);
        if (weekday >= 0) {
            // Bare/"this" weekdays include today; "next" is strictly in the future.
            const delta = (weekday - today.getDay() + 7) % 7;
            return days(delta || (match[1] === 'next' ? 7 : 0));
        }
    }
    if (text === 'next week')return days(7);
    if (text === 'next month')return months(1);
    if (text === 'next year')return months(12);
    if (text === 'end of month')return result(new Date(today.getFullYear(), today.getMonth() + 1, 0, 12));

    match = /^(?:in )?(\d+|a|an|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve) (day|week|month|year)s?(?: (from now|ago))?$/.exec(text);
    if (match) {
        const numbers = {a: 1, an: 1, one: 1, two: 2, three: 3, four: 4, five: 5, six: 6,
            seven: 7, eight: 8, nine: 9, ten: 10, eleven: 11, twelve: 12};
        const count = (numbers[match[1]] || Number(match[1])) * (match[3] === 'ago' ? -1 : 1);
        if (!Number.isSafeInteger(count) || Math.abs(count) > 3650000)return error;
        if (match[2] === 'day')return days(count);
        if (match[2] === 'week')return days(count * 7);
        return months(count * (match[2] === 'year' ? 12 : 1));
    }

    // Named months avoid guessing whether 03/04 means March 4 or April 3.
    const names = ['january', 'february', 'march', 'april', 'may', 'june',
        'july', 'august', 'september', 'october', 'november', 'december'];
    const named = text.replace(/^on /, '').replace(/,/g, '').replace(/(\d)(st|nd|rd|th)\b/g, '$1');
    match = /^([a-z]+) (\d{1,2})(?: (\d{4}))?$/.exec(named);
    if (!match) {
        const reversed = /^(\d{1,2}) ([a-z]+)(?: (\d{4}))?$/.exec(named);
        if (reversed)match = [reversed[0], reversed[2], reversed[1], reversed[3]];
    }
    if (match) {
        const month = names.findIndex(name => name === match[1] || name.slice(0, 3) === match[1]);
        if (month < 0)return error;
        const day = Number(match[2]);
        if (match[3])return result(calendar(Number(match[3]), month, day));
        // Pick the next occurrence, including leap day in the next leap year.
        for (let year = today.getFullYear(); year <= today.getFullYear() + 8; year++) {
            const date = calendar(year, month, day);
            if (date && date >= today)return result(date);
        }
    }
    return error;
}
