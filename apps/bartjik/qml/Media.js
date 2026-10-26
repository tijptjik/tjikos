// URI destinations are allowlisted before being passed as a single argv value.
function spotifyUri(value, type) {
    const text=String(value || '');
    const native=new RegExp('^spotify:'+type+':[A-Za-z0-9]+$');
    if(native.test(text)) return text;
    const web=new RegExp('^https://open\\.spotify\\.com/'+type+'/([A-Za-z0-9]+)(?:\\?[^\\s]*)?$');
    const match=text.match(web);
    return match ? 'spotify:'+type+':'+match[1] : '';
}
function artistDestination(metadata, artist) {
    return spotifyUri(metadata['spotify:artistUrl'],'artist');
}
function playlistDestination(metadata) {
    return spotifyUri(metadata['spotify:contextUrl'] || metadata['xesam:playlistUrl'],'playlist');
}
function artwork(value) {
    const text=String(value || '');
    return /^(https?:\/\/|file:\/\/\/)/.test(text) ? text : '';
}

// MPRIS/Quickshell positions are in seconds. A second back-scroll can skip.
function previousAction(position, positionSupported, canSeek) {
    return positionSupported && canSeek && Number.isFinite(position) && position>1 ? 'restart' : 'previous';
}

// Greatest timestamp <= position; works for normal playback and seeks in either direction.
function lyricLine(lines, position) {
    if(!Number.isFinite(position) || position<0)return -1;
    let lo=0,hi=lines.length;
    while(lo<hi) {
        const middle=(lo+hi)>>1;
        if(lines[middle].time<=position)lo=middle+1;
        else hi=middle;
    }
    return lo-1;
}

// Only exact Spotify recording IDs are eligible for Spotify's timestamps.
function spotifyTrackId(metadata) {
    for(const value of [metadata['xesam:url'],metadata['mpris:trackid']]) {
        const raw=String(value || '');
        // Qt exposes object-path metadata as an opaque QVariant in QML.
        const wrapped=raw.match(/^QVariant\(QDBusObjectPath, QDBusObjectPath\("(\/com\/spotify\/track\/[A-Za-z0-9]{22})"\)\)$/);
        const match=(wrapped?wrapped[1]:raw).match(/^(?:spotify:track:|\/com\/spotify\/track\/|https:\/\/open\.spotify\.com\/(?:intl-[a-zA-Z-]+\/)?track\/)([A-Za-z0-9]{22})(?:[?#][^\s]*)?$/);
        if(match)return match[1];
    }
    return '';
}
