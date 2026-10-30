import QtQuick
import Quickshell
import Quickshell.Io
import "Logic.js" as Logic

// One recorder belongs to the notification service, not to a screen or card.
Item {
    id: recorder
    required property var controller
    property bool locked: true
    property string noticeId: ""
    property string phase: "idle"
    property string source: ""
    property string token: ""
    property real duration: 0
    property string error: ""
    property bool uncertain: false
    property bool sendWhenReady: false
    property var destination: ({})
    readonly property bool recording: phase === "starting" || phase === "recording" || phase === "stopping"
    readonly property bool holdsNotice: noticeId !== "" && phase !== "idle" && phase !== "error"
    readonly property bool previewing: controller.telegramPlayer.noticeId === noticeId && controller.telegramPlayer.player.source.toString() === source && !!source
    onLockedChanged: { if (locked) { sendWhenReady = false; if (recording) stop(); } }
    Connections {
        target: recorder.controller
        function onConfigChanged() {
            const notice = recorder.controller.find(recorder.noticeId);
            if (notice && Logic.callerBlocked(notice, recorder.controller.config)) recorder.discard();
        }
    }
    function begin(notice) {
        if (locked || capture.running || send.running || (holdsNotice && noticeId !== notice.id)) return;
        controller.telegramPlayer.stop();
        noticeId = notice.id; destination = {chatId:notice.data.chatId, messageId:notice.data.messageId, demo:notice.data.demo === true};
        phase = "starting"; duration = 0; source = ""; token = ""; error = ""; uncertain = false;
        capture.running = true;
    }
    function stop() {
        if (!capture.running || !recording || phase === "stopping") return;
        phase = "stopping";
        capture.write(JSON.stringify({action:"stop"}) + "\n");
    }
    function discard() {
        sendWhenReady = false;
        if (send.running) return;
        if (previewing) controller.telegramPlayer.stop();
        if (capture.running) { phase = "discarding"; capture.write(JSON.stringify({action:"discard"}) + "\n"); }
        else reset();
    }
    function discardIf(id) { if (noticeId === id) discard(); }
    function reset() { sendWhenReady = false; phase = "idle"; noticeId = ""; source = ""; token = ""; duration = 0; error = ""; uncertain = false; }
    function submit() {
        const notice = controller.find(noticeId);
        if (locked || uncertain || !notice || Logic.callerBlocked(notice, controller.config)) return;
        if (phase === "recording" && duration > 0) { sendWhenReady = true; stop(); return; }
        if (phase !== "ready" || !token) return;
        sendWhenReady = false;
        if (previewing) controller.telegramPlayer.stop();
        phase = "sending"; error = "";
        send.running = true;
    }
    Process {
        id: capture
        command: [Quickshell.shellDir + "/../bin/notjikctl", "record"]
        stdinEnabled: true
        onStarted: {
            // The UI can stop/discard before QProcess has opened its stdin.
            if (recorder.phase === "discarding") write(JSON.stringify({action:"discard"}) + "\n");
            else if (recorder.locked || recorder.phase === "stopping") {
                recorder.phase = "stopping";
                write(JSON.stringify({action:"stop"}) + "\n");
            }
        }
        stdout: SplitParser { onRead: line => {
            try {
                const result = JSON.parse(line);
                if (recorder.phase === "discarding") return;
                if (result.duration !== undefined) recorder.duration = result.duration;
                if (result.source) recorder.source = result.source;
                if (result.token) recorder.token = result.token;
                if (result.error) recorder.error = result.error;
                if (result.state) {
                    // A lock/Stop can race the helper's initial recording event.
                    if (recorder.phase !== "stopping" || result.state !== "recording") recorder.phase = result.state;
                    if (recorder.locked && recorder.recording) recorder.stop();
                    if (result.state === "error") recorder.sendWhenReady = false;
                    if (result.state === "ready" && recorder.sendWhenReady) Qt.callLater(() => {
                        if (recorder.sendWhenReady && recorder.phase === "ready") recorder.submit();
                    });
                }
            } catch (_) { recorder.error = "Invalid recording response"; recorder.discard(); }
        } }
        onExited: (code, status) => {
            if (recorder.phase === "discarding") recorder.reset();
            else if (recorder.phase !== "error") { recorder.phase = "error"; recorder.error = "Recording ended unexpectedly; try again"; recorder.source = ""; recorder.token = ""; }
        }
    }
    Process {
        id: send
        command: [Quickshell.shellDir + "/../bin/notjikctl", "telegram", "action"]
        stdinEnabled: true
        property bool accepted: false
        onStarted: {
            accepted = false;
            write(JSON.stringify({action:recorder.destination.demo ? "demoVoiceReply" : "voiceReply", chatId:recorder.destination.chatId, messageId:recorder.destination.messageId, token:recorder.token}) + "\n");
        }
        stdout: SplitParser { onRead: line => {
            try {
                const result = JSON.parse(line);
                send.accepted = result.ok === true;
                recorder.uncertain = result.uncertain === true;
                if (!send.accepted) recorder.error = result.error || "Voice reply was not accepted";
            } catch (_) { recorder.uncertain = true; recorder.error = "No confirmation from Telegram; check the chat before retrying"; }
        } }
        onExited: (code, status) => {
            if (accepted) recorder.discard();
            else {
                recorder.phase = "ready";
                if (!recorder.error) { recorder.uncertain = true; recorder.error = "No confirmation from Telegram; check the chat before retrying"; }
                if (!recorder.controller.find(recorder.noticeId)) recorder.discard();
            }
        }
    }
}
