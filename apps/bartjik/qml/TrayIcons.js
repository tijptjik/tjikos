// Trusted configuration only. First matching key wins; matching is case-insensitive.
// Match app ID, title or tooltip title; a trailing * matches a prefix. Other * are literal.
function override(overrides, id, title, tooltip) {
    const names = [String(id || '').toLowerCase(), String(title || '').toLowerCase(), String(tooltip || '').toLowerCase()];
    for (const key of Object.keys(overrides || {})) {
        const pattern = key.toLowerCase();
        if (names.some(name => pattern.endsWith('*') ? name.startsWith(pattern.slice(0,-1)) : name === pattern))
            return overrides[key];
    }
    return '';
}

function excluded(patterns, id, title) {
    return patterns.some(pattern => !!override({[pattern]: true}, id, title));
}

// Chromium tray IDs are shared by unrelated apps; identify ChatGPT by its title.
function focusApp(id, title, tooltip) {
    const names=[id,title,tooltip].map(n=>String(n || '').toLowerCase());
    if(names.includes('chatgpt'))return 'chatgpt';
    if(names.includes('telegramdesktop'))return 'telegram';
    if(names.includes('zapzap'))return 'zapzap';
    if(names.includes('codex'))return 'codex';
    return '';
}
