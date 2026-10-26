import QtQuick
import QtQuick.Controls
import "Media.js" as Media

Item {
    id:lyrics
    required property var controller
    required property var theme
    property var track:({})
    property string playerId:''
    property bool active:false
    readonly property string trackKey:JSON.stringify([playerId,track.trackId || '',track.spotifyId || '',track.lyricsArtist || track.artist || '',track.title || '',track.album || '',Math.round(track.duration || 0)])
    readonly property var player:active?controller.mediaPlayer(playerId):null
    property int sequence:0
    property string token:''
    property string status:'loading'
    property string text:''
    property var lines:[]
    property real position:-1
    readonly property bool synced:lines.length>0 && !!player?.positionSupported
    readonly property int currentLine:synced?Media.lyricLine(lines,position):-1
    readonly property string message:status==='loading'?'Loading lyrics…':status==='instrumental'?'Instrumental':status==='error'?'Couldn’t load lyrics. Try again shortly.':'No lyrics found for this track.'

    function updatePosition() {position=player?.positionSupported?player.position:-1;}
    function reset() {
        token=String(++sequence);
        text='';lines=[];status='loading';scroll.contentY=0;
        lookup.stop();deadline.stop();resume.stop();follow.stop();
        updatePosition();
        if(active)lookup.start();
    }
    function followCurrent() {
        if(!active || !synced || resume.running || scroll.dragging || scroll.flicking || lyricScrollBar.pressed || currentLine<0)return;
        const line=verses.itemAt(currentLine);
        if(!line)return;
        const target=Math.max(0,Math.min(scroll.contentHeight-scroll.height,timed.y+line.y+line.height/2-scroll.height/2));
        follow.stop();follow.to=target;follow.start();
    }
    function pauseFollow() {if(synced){follow.stop();resume.restart();}}
    onTrackKeyChanged:reset()
    onActiveChanged:reset()
    onPlayerChanged:updatePosition()
    onCurrentLineChanged:Qt.callLater(followCurrent)
    onSyncedChanged:Qt.callLater(followCurrent)
    Timer {
        id:lookup;interval:180
        onTriggered:{deadline.restart();lyrics.controller.send({op:'lyrics',token:lyrics.token,track:{spotifyId:lyrics.track.spotifyId || '',artist:lyrics.track.lyricsArtist || lyrics.track.artist || '',title:lyrics.track.title || '',album:lyrics.track.album || '',duration:lyrics.track.duration || 0}});}
    }
    Timer {id:deadline;interval:30000;onTriggered:if(lyrics.active && lyrics.status==='loading')lyrics.status='error'}
    Timer {
        id:playback;interval:100;repeat:true
        running:lyrics.active && lyrics.synced && !!lyrics.player?.isPlaying
        onTriggered:lyrics.updatePosition()
    }
    Timer {id:resume;interval:4000;onTriggered:lyrics.followCurrent()}
    Connections {
        target:lyrics.player
        function onPositionChanged(){lyrics.updatePosition();}
        function onPositionSupportedChanged(){lyrics.updatePosition();}
        function onIsPlayingChanged(){lyrics.updatePosition();}
    }
    Connections {
        target:lyrics.controller
        function onLyricsReceived(token,result) {
            if(!lyrics.active || token!==lyrics.token)return;
            deadline.stop();lyrics.text=result.text || '';lyrics.lines=result.lines || [];lyrics.status=result.status || 'error';
            lyrics.updatePosition();Qt.callLater(lyrics.followCurrent);
        }
    }
    NumberAnimation {id:follow;target:scroll;property:'contentY';duration:220;easing.type:Easing.InOutQuad}
    Flickable {
        id:scroll;anchors.fill:parent;anchors.margins:22
        visible:lyrics.status==='ready';clip:true
        readonly property real padding:lyrics.synced?Math.max(0,(height-24)/2):0
        contentWidth:width;contentHeight:lyrics.lines.length?timed.implicitHeight+padding*2:words.implicitHeight
        boundsBehavior:Flickable.StopAtBounds
        onDraggingChanged:if(dragging)lyrics.pauseFollow()
        onMovementEnded:if(lyrics.synced)resume.restart()
        onWidthChanged:Qt.callLater(lyrics.followCurrent)
        onHeightChanged:Qt.callLater(lyrics.followCurrent)
        Text {
            id:words;width:scroll.width-10;visible:!lyrics.lines.length
            text:lyrics.text;textFormat:Text.PlainText;wrapMode:Text.Wrap
            font.family:lyrics.theme.font;font.pixelSize:16;lineHeight:1.35
            color:lyrics.theme.text;horizontalAlignment:Text.AlignHCenter
        }
        Column {
            id:timed;y:scroll.padding;width:scroll.width-10;spacing:12;visible:lyrics.lines.length>0
            onImplicitHeightChanged:Qt.callLater(lyrics.followCurrent)
            Repeater {
                id:verses;model:lyrics.lines
                Text {
                    required property var modelData
                    required property int index
                    readonly property bool highlighted:lyrics.currentLine===index && !!modelData.text
                    width:timed.width
                    text:modelData.text || ' ';textFormat:Text.PlainText;wrapMode:Text.Wrap
                    font.family:lyrics.theme.font;font.pixelSize:16;font.bold:highlighted;lineHeight:1.35
                    color:highlighted?lyrics.theme.accent:lyrics.theme.text
                    opacity:!lyrics.synced || highlighted?1:.5
                    horizontalAlignment:Text.AlignHCenter
                    Behavior on color {ColorAnimation {duration:140}}
                    Behavior on opacity {NumberAnimation {duration:140}}
                }
            }
        }
        ScrollBar.vertical:ScrollBar {id:lyricScrollBar;policy:ScrollBar.AsNeeded;onPressedChanged:lyrics.pauseFollow()}
    }
    MouseArea {
        anchors.fill:scroll;acceptedButtons:Qt.NoButton
        onWheel:wheel=>{lyrics.pauseFollow();wheel.accepted=false;}
    }
    Text {
        anchors.centerIn:parent;width:parent.width-48
        visible:lyrics.status!=='ready';text:lyrics.message
        textFormat:Text.PlainText;wrapMode:Text.Wrap;horizontalAlignment:Text.AlignHCenter
        font.family:lyrics.theme.font;font.pixelSize:14;color:lyrics.theme.muted
    }
}
