// Shared by the native and browser editors. Only supported recurrence rules are
// accepted; unsupported language remains editable rather than being guessed.
function format(rule) {
    if (!rule || !rule.unit)return 'Does not repeat';
    const value = String(rule.value === undefined ? 1 : rule.value);
    const names = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];
    if (rule.unit === 'days-of-the-week') {
        const days = value.split(',').map(Number).sort((a,b)=>a-b);
        if (days.join(',') === '1,2,3,4,5')return 'Every weekday';
        if (days.join(',') === '6,7')return 'Every weekend';
        return 'Every ' + days.map(n=>names[n-1]).join(', ');
    }
    if (rule.unit === 'days-of-the-month') {
        function ordinal(n) {return n + (n%100>=11 && n%100<=13?'th':n%10===1?'st':n%10===2?'nd':n%10===3?'rd':'th');}
        return 'On the ' + value.split(',').map(Number).sort((a,b)=>a-b).map(ordinal).join(', ') + ' of every month';
    }
    return Number(value) === 1 ? 'Every ' + rule.unit.slice(0,-1) : 'Every ' + value + ' ' + rule.unit;
}

function parse(input) {
    let text = input.trim().toLowerCase().replace(/\s+/g, ' ');
    const error = {rule:null, label:'', error:'Repeat not understood. Try “every 2 weeks”, “every Monday and Friday”, or “on the 1st and 15th of every month”.'};
    function result(unit, value) {
        const rule = unit ? {unit:unit, value:value} : {};
        return {rule:rule, label:format(rule), error:''};
    }
    if (!text || ['none','never','once','no repeat','does not repeat','do not repeat'].includes(text))return result('', 0);
    const aliases = {daily:'every day',weekly:'every week',monthly:'every month',yearly:'every year',annually:'every year',fortnightly:'every 2 weeks',weekdays:'every weekday',weekends:'every weekend'};
    text = aliases[text] || text;
    const words = {a:1,an:1,one:1,two:2,three:3,four:4,five:5,six:6,seven:7,eight:8,nine:9,ten:10,eleven:11,twelve:12};
    let match = /^(?:every|each) (?:(other|\d+|a|an|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve) )?(day|week|month|year)s?$/.exec(text);
    if (match) {
        const n = !match[1] ? 1 : match[1]==='other'?2:words[match[1]] || Number(match[1]);
        return Number.isInteger(n) && n>=1 && n<=10000 ? result(match[2]+'s',n) : error;
    }
    if (/^(?:every|each) weekdays?$/.test(text))return result('days-of-the-week','1,2,3,4,5');
    if (/^(?:every|each) weekends?$/.test(text))return result('days-of-the-week','6,7');
    const weekdays = {mon:1,monday:1,tue:2,tues:2,tuesday:2,wed:3,wednesday:3,thu:4,thur:4,thurs:4,thursday:4,fri:5,friday:5,sat:6,saturday:6,sun:7,sunday:7};
    let dayText = text.replace(/^(?:every|each|on) /, '').replace(/ every week$/, '');
    let tokens = dayText.replace(/ and /g, ',').split(/\s*,\s*/);
    if (tokens.length && tokens.every(t=>weekdays[t]))return result('days-of-the-week',Array.from(new Set(tokens.map(t=>weekdays[t]))).sort((a,b)=>a-b).join(','));
    match = /^(?:on )?(?:the )?(.+?) (?:of (?:every|each|the) month|every month|each month|monthly)$/.exec(text);
    if (!match)match = /^(?:every|each) month on (?:the )?(.+)$/.exec(text);
    if (match) {
        tokens = match[1].replace(/ and /g, ',').split(/\s*,\s*/);
        if (tokens.every(t=>/^\d{1,2}(?:st|nd|rd|th)?$/.test(t))) {
            const days = tokens.map(t=>parseInt(t,10));
            if (days.every(n=>n>=1 && n<=31))return result('days-of-the-month',Array.from(new Set(days)).sort((a,b)=>a-b).join(','));
        }
    }
    return error;
}
