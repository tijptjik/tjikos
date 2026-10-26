// Weekly reset time is independent of remaining request capacity.
function countdown(resetAt, now) {
    if (typeof resetAt !== 'number' || !isFinite(resetAt))
        return {known:false, hours:0, label:'Unknown'};
    const seconds = Math.max(0, resetAt - now);
    const minutes = Math.ceil(seconds / 60);
    const days = Math.floor(minutes / 1440);
    const hours = Math.floor(minutes % 1440 / 60);
    const rest = minutes % 60;
    return {
        known:true,
        hours:Math.min(168, seconds / 3600),
        label:seconds <= 0 ? 'Due now' : days > 0 ? days + 'd ' + hours + 'h'
            : hours > 0 ? hours + 'h ' + rest + 'm' : rest + 'm'
    };
}
function dayHours(hours, day) {
    return Math.max(0, Math.min(24, hours - day * 24));
}
// Both resources occupy the same weekly scale, split into seven equal day slots.
function comparison(hours, credit, day) {
    if (typeof hours !== 'number' || !isFinite(hours)
            || typeof credit !== 'number' || !isFinite(credit) || credit < 0)
        return {known:false, overlap:0, timeOnly:0, creditOnly:0, empty:1};
    const time = dayHours(hours, day) / 24;
    const capacity = Math.max(0, Math.min(1, Math.min(1, credit) * 7 - day));
    return {known:true, overlap:Math.min(time, capacity),
        timeOnly:Math.max(0, time-capacity), creditOnly:Math.max(0, capacity-time),
        empty:1-Math.max(time, capacity)};
}
function remainingLabel(credit) {
    if (typeof credit !== 'number' || !isFinite(credit) || credit < 0) return '—';
    return (Math.round(Math.min(1,credit)*1000)/10) + '%';
}
