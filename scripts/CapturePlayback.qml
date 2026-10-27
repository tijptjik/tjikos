import QtQuick
import QtMultimedia
import Quickshell
Item {
    id:capture
    property var metadata:({})
    readonly property bool playing:player.playbackState===MediaPlayer.PlayingState
    readonly property int position:player.position
    property real energy:0
    property double epoch:0
    function toggle(){if(playing)player.pause();else player.play();}
    function play(){player.play();}
    function pause(){player.pause();}
    MediaPlayer {
        id:player
        source:Quickshell.env('BARTJIK_CAPTURE_AUDIO')
        audioOutput:AudioOutput {muted:true}
        loops:MediaPlayer.Infinite
        onPlaybackStateChanged:if(capture.playing)capture.epoch=Date.now()-position
        onPositionChanged:if(capture.playing)capture.epoch=Date.now()-position
    }
    Timer {
        interval:25;running:capture.playing;repeat:true
        onTriggered:{
            const envelope=capture.metadata.envelope;
            const values=envelope?.values || [];
            const elapsed=(Date.now()-capture.epoch)%Math.max(1,player.duration);
            capture.energy=values.length?values[Math.floor(elapsed/(envelope.stepMs || 25))%values.length]:0;
        }
    }
    onPlayingChanged:if(!playing)energy=0
}
