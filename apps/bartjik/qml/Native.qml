import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Mpris
import Quickshell.Services.SystemTray
import "TrayIcons.js" as TrayIcons
import "Media.js" as Media
import "Logic.js" as Logic

QtObject {
    id: native
    property bool demo: false
    // These singletons are shared by every renderer; no per-bar service objects.
    readonly property var players: demo ? [] : Mpris.players.values
    readonly property var tray: demo ? [] : SystemTray.items.values
    property var attention: ({})
    property Process attentionReader: Process {
        running: !native.demo
        command: ['/usr/bin/python3', '-B', Qt.resolvedUrl('tray_attention.py').toString().replace('file://','')]
        stdout: SplitParser {onRead: data => {try {native.attention=Logic.reconcile(native.attention,JSON.parse(data));} catch(e) {console.warn('tray attention: invalid update');}}}
        onExited: (code,status) => {native.attention={};if(!native.demo){console.warn('tray attention reader exited: '+code);native.attentionRetry.restart();}}
    }
    property Timer attentionRetry: Timer {interval:5000;onTriggered:native.attentionReader.running=!native.demo}
    property string navigationError: ''
    property var navigationQueue: []
    property var navigationRequest: ({})
    property Process mediaNavigation: Process {
        command: ['/usr/bin/python3','-B',Qt.resolvedUrl('media_navigate.py').toString().replace('file://','')]
        onStarted: {write(JSON.stringify(native.navigationRequest));stdinEnabled=false;}
        stderr: StdioCollector {onStreamFinished:if(text.trim())native.navigationError=text.trim();}
        onExited: (code,status) => {
            if(code!==0 && !native.navigationError)native.navigationError='Spotify navigation failed';
            native.nextNavigation();
        }
    }
    function nextNavigation() {
        if(mediaNavigation.running || !navigationQueue.length)return;
        navigationRequest=navigationQueue[0];navigationQueue=navigationQueue.slice(1);
        navigationError='';mediaNavigation.stdinEnabled=true;mediaNavigation.running=true;
    }
    function navigate(action,p) {
        if(!/^org\.mpris\.MediaPlayer2\.spotify(?:\.|$)/.test(p.dbusName)) {
            if(p.canRaise)p.raise();
            return;
        }
        if(navigationQueue.length>=4){navigationError='Spotify navigation is busy';return;}
        const target=action==='player.artist'?'artist':'playlist';
        navigationQueue=navigationQueue.concat([{target:target,player:p.dbusName,trackId:Media.spotifyTrackId(p.metadata),
            uri:target==='artist'?Media.artistDestination(p.metadata):Media.playlistDestination(p.metadata)}]);
        nextNavigation();
    }
    property var focusQueue: []
    property string focusApp: ''
    property Process appFocus: Process {
        command: ['/usr/bin/python3','-B',Qt.resolvedUrl('tray_focus.py').toString().replace('file://',''),native.focusApp]
        onExited: (code,status) => {if(code!==0)console.warn('tray app focus failed: '+code);native.nextFocus();}
    }
    function nextFocus() {
        if(appFocus.running || !focusQueue.length)return;
        focusApp=focusQueue[0];focusQueue=focusQueue.slice(1);appFocus.running=true;
    }
    property var trayRecords: []
    property int traySequence: 0
    property string menuItemId: ""
    onTrayChanged: {
        const old=trayRecords;
        trayRecords=tray.map(t=>old.find(r=>r.object===t) || {object:t,id:'sni-'+(++traySequence)});
        if(menuItemId && !trayRecords.some(r=>r.id===menuItemId))menuPopup.closeMenu();
    }
    property TrayMenu menuPopup: TrayMenu {}
    function base(id, type, icon, text, tooltip, actions) {
        return {id:id,type:type,icon:{kind:'symbol',value:icon,style:{},actions:{}},
            text:{value:text,style:{},actions:{}},tooltip:tooltip,state:{},visible:true,enabled:true,actions:actions};
    }
    function items(module) {
        if(module.provider === 'icontray') return trayRecords.filter(r => r.object && tray.indexOf(r.object)>=0 && !TrayIcons.excluded(module.exclude || ['Input Method'],r.object.id,r.object.title)).map(r => {
            const t=r.object;
            const row=base(r.id,'icon',t.icon,'',t.tooltipTitle + (t.tooltipDescription ? '\n'+t.tooltipDescription:''),
                {leftClick:'tray.activate',rightClick:'tray.menu',middleClick:'tray.secondary',scrollUp:'tray.up',scrollDown:'tray.down'});
            row.icon.kind='native';
            const artwork=TrayIcons.override(module.iconOverrides,t.id,t.title,t.tooltipTitle);
            if(artwork) {
                row.icon.kind='image';
                row.icon.value=artwork.startsWith('builtin:') ? Qt.resolvedUrl('icons/tray/'+artwork.slice(8)+'.svg').toString() : artwork;
            }
            row.state={urgent:t.status===Status.NeedsAttention || attention[t.id]===true};return row;
        });
        if(module.provider === 'playerctl') {
            const allowed=players.filter(p => !(module.players || []).length || module.players.some(name => p.identity.toLowerCase()===name.toLowerCase() || p.desktopEntry.toLowerCase()===name.toLowerCase() || p.dbusName.toLowerCase().indexOf('.'+name.toLowerCase())>=0));
            const p=allowed.find(p => p.isPlaying) || allowed[0];
            if(!p)return [];
            const row=base(p.dbusName,'icon_text',p.isPlaying?'pause':'play_arrow',p.trackTitle || p.identity,'',
                {middleClick:'player.toggle',scrollUp:'player.previous',scrollDown:'player.next'});
            row.icon.actions={leftClick:'player.toggle'};
            row.artist={value:p.trackArtist || p.trackAlbumArtist || '',style:{},actions:{leftClick:'player.artist'}};
            row.track={value:p.trackTitle || p.identity,style:{},actions:{leftClick:'player.playlist'}};
            row.media={artist:row.artist.value,title:p.trackTitle || '',album:p.trackAlbum,artwork:Media.artwork(p.trackArtUrl),
                trackId:String(p.metadata['mpris:trackid'] || ''),spotifyId:Media.spotifyTrackId(p.metadata),duration:p.length,
                lyricsArtist:String(p.metadata['xesam:artist']?.[0] || row.artist.value),
                playing:p.isPlaying,canToggle:p.canTogglePlaying,canPrevious:p.canGoPrevious || p.canSeek,canNext:p.canGoNext,
                canShuffle:p.shuffleSupported && p.canControl,shuffle:p.shuffle,
                playlistAvailable:!!Media.playlistDestination(p.metadata),player:p.identity};
            return [row];
        }
        return [];
    }
    function snapshot() {
        return {players:players.map(p=>({id:p.dbusName,identity:p.identity,playing:p.isPlaying,canControl:p.canControl,canQuit:p.canQuit,position:p.position,canSeek:p.canSeek,shuffle:p.shuffle,shuffleSupported:p.shuffleSupported})),
            tray:trayRecords.filter(r=>r.object && tray.indexOf(r.object)>=0).map(r=>({id:r.id,appId:r.object.id,title:r.object.title,status:r.object.status,hasMenu:r.object.hasMenu}))};
    }
    function act(action, id, window, rect) {
        if(action.startsWith('tray.')) {
            const t=trayRecords.find(r=>r.id===id)?.object;if(!t)return false;
            const app=TrayIcons.focusApp(t.id,t.title,t.tooltipTitle);
            if(action==='tray.activate' && app) {
                focusQueue=focusQueue.concat([app]);nextFocus();
            } else if(action==='tray.menu' || (action==='tray.activate' && t.onlyMenu)) {
                if(t.hasMenu) {menuItemId=id;menuPopup.openMenu(t.menu,window,rect,window.bar?.style);}
            } else if(action==='tray.activate')t.activate();
            else if(action==='tray.secondary')t.secondaryActivate();
            else if(action==='tray.up')t.scroll(120,false);
            else if(action==='tray.down')t.scroll(-120,false);
            return true;
        }
        if(action.startsWith('player.')) {
            const p=players.find(p=>p.dbusName===id);if(!p)return false;
            if(action==='player.toggle' && p.canTogglePlaying)p.togglePlaying();
            else if(action==='player.next' && p.canGoNext)p.next();
            else if(action==='player.previous') {
                if(Media.previousAction(p.position,p.positionSupported,p.canSeek)==='restart')p.position=0;
                else if(p.canGoPrevious)p.previous();
            } else if(action==='player.shuffle' && p.shuffleSupported && p.canControl)p.shuffle=!p.shuffle;
            else if(action==='player.quit' && p.canQuit)p.quit();
            else if(action==='player.artist' || action==='player.playlist')navigate(action,p);
            return true;
        }
        return false;
    }
}
