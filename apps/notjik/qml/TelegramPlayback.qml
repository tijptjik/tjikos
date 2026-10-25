import QtQuick
import QtMultimedia

Item {
    id: playback
    property bool locked: true
    property string noticeId: ""
    readonly property alias player: player
    readonly property bool playing: player.playbackState === MediaPlayer.PlayingState
    property string error: ""
    onLockedChanged: { if (locked) stop(); }

    function toggle(id, source) {
        if (locked || !source || !source.startsWith("file:///")) return;
        if (noticeId !== id || player.source.toString() !== source) {
            player.stop(); noticeId = id; player.source = source; error = "";
        }
        if (playing) player.pause();
        else {
            if (player.mediaStatus === MediaPlayer.EndOfMedia) player.position = 0;
            player.play();
        }
    }
    function stop() { player.stop(); noticeId = ""; player.source = ""; error = ""; }
    function stopIf(id) { if (noticeId === id) stop(); }
    MediaPlayer {
        id: player
        audioOutput: AudioOutput {}
        onErrorOccurred: (code, message) => { playback.error = "Unable to play this recording"; }
    }
}
