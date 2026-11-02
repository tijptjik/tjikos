// Shared compact labels for task and subtask date columns.
function dateCode(value, todayValue, view) {
    if (!value) return '·';
    const date = new Date(value + 'T12:00:00');
    const today = new Date(todayValue + 'T12:00:00');
    const delta = Math.round((date - today) / 86400000);
    const weekday = (today.getDay() + 6) % 7;
    if (view === 'today' || view === 'week' || (delta >= -weekday && delta <= 6 - weekday))
        return ['SUN', 'MON', 'TUE', 'WED', 'THU', 'FRI', 'SAT'][date.getDay()];
    const days = Math.abs(delta);
    if (days < 10) return days + 'D';
    const start = delta < 0 ? date : today, end = delta < 0 ? today : date;
    const months = (end.getFullYear() - start.getFullYear()) * 12 + end.getMonth() - start.getMonth() - (end.getDate() < start.getDate() ? 1 : 0);
    if (months < 3) return Math.max(1, Math.floor(days / 7)) + 'W';
    if (months < 12) return months + 'M';
    return Math.floor(months / 12) + 'Y';
}
