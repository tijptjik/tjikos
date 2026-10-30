import QtQuick
import QtMultimedia
import QtQuick.Controls
import QtQuick.Layouts
import Qt5Compat.GraphicalEffects
import Quickshell
import Quickshell.Io
import ".."

ColumnLayout {
    id: detail
    property bool showBody: true
    required property var notice
    required property var theme
    required property double now
    required property var controller
    property bool historyMode: false
    property bool controlsRevealed: false
    readonly property bool hasInteractions: interactive
    readonly property bool interactionHeld: interactive && (replying || recordingHere || command.running)
    readonly property var mediaData: notice.data || ({})
    readonly property var playback: controller.telegramPlayer
    readonly property var recorder: controller.telegramRecorder
    readonly property bool recordingHere: recorder.noticeId === notice.id && recorder.phase !== "idle"
    readonly property bool current: playback.noticeId === notice.id && playback.player.source.toString() === (mediaData.source || "")
    readonly property bool interactive: !notice.archived && !historyMode && !controller.locked
    // A layout first measured inside a hidden panel can retain its old implicit
    // height after every child collapses. Report content independently of that
    // measurement so an empty structured-message detail reserves no space.
    readonly property bool hasContent: (showBody && (!!notice.body || !!mediaData.parts?.length))
        || (mediaData.kind === "photo" && !!mediaData.source)
        || ["sticker", "emoji", "animation", "voice"].includes(mediaData.kind)
        || !!demoTranscript || !!mediaData.transcript || !!mediaData.transcriptionError
        || !!(current && playback.error ? playback.error : mediaData.status)
        || controlsSection.reveal > 0 || (interactive && (recordingHere || replying))
        || (!recordingHere && !!mediaData.voiceReplyStatus) || !!feedback
    property string previousNoticeId: ""
    onNoticeChanged: {
        if (previousNoticeId && previousNoticeId !== notice.id) playback.stopIf(previousNoticeId);
        previousNoticeId = notice.id;
    }
    property bool replying: false
    property bool quoteReply: false
    property var replyTarget: null
    function compose(quoted) {
        if (replying && quoteReply === quoted) { replying = false; return; }
        feedback = "";
        quoteReply = quoted;
        replyTarget = {chatId:mediaData.chatId, messageId:mediaData.messageId,
            title:notice.title, body:notice.body || mediaData.caption || (mediaData.kind === "voice" ? "Voice message" : "Media message")};
        replying = true;
        controller.selectedId = notice.id;
        controller.keyboardMonitor = monitorName || controller.targetMonitor();
        controller.keyboardMode = true;
        Qt.callLater(() => reply.forceActiveFocus());
    }
    property string feedback: ""
    property string demoTranscript: ""
    property var pendingRequest: ({})
    property string monitorName: ""
    readonly property bool busy: command.running
    function saveState() { return {replying: replying, quoteReply:quoteReply, replyTarget:replyTarget, text: reply.text, feedback: feedback, demoTranscript: demoTranscript}; }
    function restoreState(state) { replying = !!state.replying; quoteReply = !!state.quoteReply; replyTarget = state.replyTarget || null; reply.text = state.text || ""; feedback = state.feedback || ""; demoTranscript = state.demoTranscript || ""; }
    component TelegramVisual: Item {
        id: visual
        required property string source
        required property string format
        property bool active: false
        readonly property bool ready: format === "video" ? video.hasVideo : format === "gif" ? animation.status === Image.Ready : still.status === Image.Ready
        readonly property bool failed: format === "video" ? video.error !== MediaPlayer.NoError : format === "gif" ? animation.status === Image.Error : still.status === Image.Error
        readonly property int frame: animation.currentFrame
        readonly property int position: video.position
        Image {
            id: still
            anchors.fill: parent
            visible: visual.format !== "video" && visual.format !== "gif"
            source: visible ? visual.source : ""
            asynchronous: true
            fillMode: Image.PreserveAspectFit
        }
        AnimatedImage {
            id: animation
            anchors.fill: parent
            visible: visual.format === "gif"
            source: visible ? visual.source : ""
            asynchronous: true
            playing: visual.active
            paused: !visual.active
            fillMode: Image.PreserveAspectFit
            cache: false
        }
        VideoOutput {
            id: output
            anchors.fill: parent
            visible: visual.format === "video"
            fillMode: VideoOutput.PreserveAspectFit
        }
        MediaPlayer {
            id: video
            source: visual.format === "video" ? visual.source : ""
            videoOutput: output
            // Deliberately no audio output: GIF-like animations are always silent.
            loops: MediaPlayer.Infinite
            onMediaStatusChanged: if (mediaStatus === MediaPlayer.LoadedMedia && visual.active) play()
        }
        onActiveChanged: {
            if (format === "video") {
                if (active && source) video.play();
                else video.pause();
            }
        }
    }
    component QuietButton: ActionButton {
        theme: detail.theme
        opacity: detail.controlsRevealed ? 1 : 0
        enabled: detail.controlsRevealed
    }
    component IconButton: QuietButton {
        id: iconButton
        required property string iconName
        implicitWidth: 30
        implicitHeight: 30
        padding: 5
        leftPadding: 5; rightPadding: 5; topPadding: 5; bottomPadding: 5


        contentItem: Image {
            source: Qt.resolvedUrl("../icons/actions/" + iconButton.iconName + ".svg")
            sourceSize { width: 24; height: 24 }
            fillMode: Image.PreserveAspectFit
            layer.enabled: true
            layer.effect: ColorOverlay { color: !iconButton.enabled ? detail.theme.border : iconButton.hovered || iconButton.visualFocus ? detail.theme.text : detail.theme.muted }
        }
    }

    spacing: 10

    function stamp(milliseconds) {
        const seconds = Math.max(0, Math.floor(milliseconds / 1000));
        return Math.floor(seconds / 60) + ":" + String(seconds % 60).padStart(2, "0");
    }
    function finishReply() {
        reply.text = "";
        replying = false;
        const owner = controller, id = notice.id;
        Qt.callLater(() => owner.close(id, false));
    }
    function action(name, text) {
        if (!interactive || command.running) return;
        if (mediaData.demo === true) {
            if (name === "reply") { finishReply(); return; }
            if (name === "transcribe") demoTranscript = "See you at the café tomorrow. This is a synthetic demo transcript.";
            feedback = "Demo only · no Telegram action sent";
            return;
        }
        const target = name === "reply" && replyTarget ? replyTarget : mediaData;
        pendingRequest = {action:name, chatId:target.chatId, messageId:target.messageId, text:text || ""};
        if (name === "reply") pendingRequest.quote = quoteReply;
        feedback = "";
        command.running = true;
    }
    Process {
        id: command
        command: [Quickshell.shellDir + "/../bin/notjikctl", "telegram", "action"]
        stdinEnabled: true
        onStarted: write(JSON.stringify(detail.pendingRequest) + "\n")
        stdout: SplitParser {
            onRead: line => {
                try {
                    const result = JSON.parse(line);
                    if (result.ok) {
                        if (detail.pendingRequest.action === "reply") {
                            detail.finishReply();
                        } else if (detail.pendingRequest.action === "open") {
                            const owner = detail.controller, id = detail.notice.id;
                            Qt.callLater(() => owner.finishTelegramOpen(id));
                        } else if (detail.pendingRequest.action === "transcribe") detail.feedback = "";
                        else detail.feedback = result.message || (detail.pendingRequest.action === "read" ? "Marked as read" : "Opening Telegram…");
                    } else detail.feedback = result.error || "Telegram action failed";
                } catch (_) { detail.feedback = "Telegram bridge returned an invalid response"; }
            }
        }
        onExited: (code, status) => { if (code && !detail.feedback) detail.feedback = "Telegram bridge is unavailable"; }
    }

    Rectangle {
        id: photoArea
        Layout.fillWidth: true
        Layout.preferredHeight: visible ? width * 0.61 : 0
        visible: detail.mediaData.kind === "photo" && !!detail.mediaData.source
        radius: 8
        color: detail.theme.surface
        clip: true
        Image {
            id: photo
            anchors.fill: parent
            source: detail.mediaData.kind === "photo" ? detail.mediaData.source || "" : ""
            sourceSize { width: 1280; height: 1280 }
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
            cache: false
            layer.enabled: true
            layer.effect: OpacityMask { maskSource: Rectangle { width: photoArea.width; height: photoArea.height; radius: 8 } }
            Accessible.name: "Telegram photo preview"
        }
        Text {
            anchors.centerIn: parent
            visible: photo.status === Image.Error
            text: "Photo no longer cached"
            color: detail.theme.muted
            font { family: detail.theme.font; pixelSize: detail.theme.fontSize - 2 }
        }
        MouseArea {
            anchors.fill: parent
            enabled: detail.controlsRevealed && detail.interactive
            cursorShape: Qt.PointingHandCursor
            onClicked: detail.action("open", "")
        }
    }

    Item {
        Layout.fillWidth: true
        Layout.preferredHeight: visible ? (detail.mediaData.kind === "emoji" ? 96 : Math.min(240, width * (detail.mediaData.height || 200) / Math.max(1, detail.mediaData.width || 320))) : 0
        visible: ["sticker", "emoji", "animation"].includes(detail.mediaData.kind)
        TelegramVisual {
            id: richMedia
            anchors.fill: parent
            source: detail.mediaData.source || ""
            format: detail.mediaData.format || ""
            active: detail.visible && !detail.controller.locked && !detail.controller.paused && !detail.historyMode
            Accessible.name: detail.mediaData.kind === "sticker" ? "Sticker" : detail.mediaData.kind === "emoji" ? "Animated emoji" : "Animation"
        }
        Text {
            anchors.centerIn: parent
            visible: !richMedia.ready
            text: detail.mediaData.fallback || (richMedia.failed ? "Animation unavailable" : "")
            textFormat: Text.PlainText
            color: detail.theme.muted
            font.pixelSize: detail.mediaData.kind === "emoji" ? 48 : 24
        }
    }

    RowLayout {
        Layout.fillWidth: true
        visible: detail.mediaData.kind === "voice"
        spacing: 12
        Button {
            id: play
            objectName: "telegram-play"
            implicitWidth: 42
            implicitHeight: 42
            enabled: detail.interactive && !detail.recorder.recording && !!detail.mediaData.source
            hoverEnabled: true
            Accessible.name: detail.current && detail.playback.playing ? "Pause voice message" : "Play voice message"


            background: Rectangle {
                radius: width / 2
                color: play.enabled ? (play.down ? Qt.darker(detail.theme.accent, 1.2) : detail.theme.accent) : detail.theme.surface
                border.width: play.visualFocus ? 2 : 0
                border.color: detail.theme.text
            }
            contentItem: Canvas {
                id: playGlyph
                readonly property bool pausedGlyph: detail.current && detail.playback.playing
                onPausedGlyphChanged: requestPaint()
                onPaint: {
                    const ctx = getContext("2d");
                    ctx.clearRect(0, 0, width, height);
                    ctx.fillStyle = "#191919";
                    const cx = width / 2, cy = height / 2;
                    if (pausedGlyph) { ctx.fillRect(cx-6, cy-7, 4, 14); ctx.fillRect(cx+2, cy-7, 4, 14); }
                    else { ctx.beginPath(); ctx.moveTo(cx-4, cy-8); ctx.lineTo(cx+8, cy); ctx.lineTo(cx-4, cy+8); ctx.closePath(); ctx.fill(); }
                }
            }
            onClicked: detail.playback.toggle(detail.notice.id, detail.mediaData.source)
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0
            Slider {
                id: seek
                objectName: "telegram-seek"
                Layout.fillWidth: true
                implicitHeight: 30
                from: 0
                to: detail.current ? Math.max(1, detail.playback.player.duration) : Math.max(1, (detail.mediaData.duration || 0) * 1000)
                value: detail.current ? detail.playback.player.position : 0
                enabled: detail.controlsRevealed && detail.interactive && detail.current && detail.playback.player.seekable
                hoverEnabled: true
                Accessible.name: "Voice message playback position"
                onMoved: detail.playback.player.position = value
                background: Item {
                    x: seek.leftPadding
                    y: seek.topPadding
                    width: seek.availableWidth
                    height: seek.availableHeight
                    readonly property var samples: detail.mediaData.waveform || []
                    readonly property int barCount: samples.length ? Math.min(samples.length, Math.max(1, Math.floor(width / 4))) : 0
                    Row {
                        anchors.fill: parent
                        spacing: 2
                        Repeater {
                            model: parent.parent.barCount
                            Rectangle {
                                required property int index
                                readonly property var peaks: detail.mediaData.waveform || []
                                readonly property int count: parent.parent.barCount
                                readonly property real peak: {
                                    const start = Math.floor(index * peaks.length / count);
                                    const end = Math.floor((index + 1) * peaks.length / count);
                                    let value = 0;
                                    for (let i = start; i < end; ++i) value = Math.max(value, peaks[i]);
                                    return value;
                                }
                                width: (parent.width - Math.max(0, count - 1) * 2) / Math.max(1, count)
                                height: Math.max(2, peak / 31 * parent.height)
                                y: (parent.height - height) / 2
                                radius: width / 2
                                color: (index + 0.5) / count <= seek.position ? detail.theme.accent : detail.theme.muted
                            }
                        }
                    }
                    Rectangle {
                        visible: parent.barCount === 0
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width; height: 4; radius: 2; color: detail.theme.border
                        Rectangle { width: seek.position * parent.width; height: parent.height; radius: 2; color: detail.theme.accent }
                    }
                }
                handle: Rectangle {
                    x: seek.leftPadding + seek.visualPosition * (seek.availableWidth - width)
                    y: seek.topPadding + seek.availableHeight / 2 - height / 2
                    width: 10; height: 10; radius: 5
                    color: detail.theme.accent
                    opacity: seek.enabled && (seek.hovered || seek.pressed || seek.visualFocus) ? 1 : 0
                }
            }
            RowLayout {
                Layout.fillWidth: true
                Label {
                    Layout.fillWidth: true
                    text: detail.stamp(detail.current ? detail.playback.player.position : 0) + " / " +
                        detail.stamp(detail.current && detail.playback.player.duration > 0 ? detail.playback.player.duration : (detail.mediaData.duration || 0) * 1000)
                    color: detail.theme.muted
                    font { family: detail.theme.font; pixelSize: detail.theme.fontSize - 3 }
                }
                QuietButton {
                    id: transcribe
                    objectName: "telegram-transcribe"
                    text: "→A"
                    implicitWidth: 28
                    implicitHeight: 22
                    Accessible.name: detail.mediaData.transcriptionState === "pending" ? "Transcribing…" :
                        detail.mediaData.transcriptionState === "done" || detail.demoTranscript ? "Transcribed" : "Transcribe"


                    enabled: detail.controlsRevealed && detail.interactive && !command.running && detail.mediaData.transcriptionState !== "pending"
                             && detail.mediaData.transcriptionState !== "done" && !detail.demoTranscript
                    onClicked: detail.action("transcribe", "")
                }
                QuietButton {
                    objectName: "telegram-speed"
                    text: detail.playback.player.playbackRate + "×"
                    implicitHeight: 22
                    enabled: detail.controlsRevealed && detail.interactive
                    Accessible.name: "Playback speed " + text
                    onClicked: {
                        const rates = [1, 1.25, 1.5, 2];
                        detail.playback.player.playbackRate = rates[(rates.indexOf(detail.playback.player.playbackRate) + 1) % rates.length];
                    }
                }
            }
        }
    }

    Text {
        Layout.fillWidth: true
        text: detail.demoTranscript || detail.mediaData.transcript || ""
        visible: text.length > 0
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        color: detail.theme.text
        font { family: detail.theme.font; pixelSize: detail.theme.fontSize }
    }
    Label {
        Layout.fillWidth: true
        text: detail.mediaData.transcriptionError || ""
        visible: text.length > 0
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        color: detail.theme.muted
        font { family: detail.theme.font; pixelSize: detail.theme.fontSize - 2 }
    }

    Flow {
        id: inlineEmoji
        Layout.fillWidth: true
        visible: detail.showBody && !!detail.mediaData.parts?.length
        spacing: 0
        Repeater {
            model: detail.mediaData.parts || []
            Item {
                required property var modelData
                width: modelData.text === "\n" ? inlineEmoji.width : modelData.kind === "emoji" ? 24 : Math.min(inlineEmoji.width, fragment.implicitWidth)
                height: Math.max(24, fragment.implicitHeight)
                Text {
                    id: fragment
                    anchors.fill: parent
                    visible: modelData.kind !== "emoji" || !emojiLoader.item?.ready
                    text: modelData.text === "\n" ? "" : (modelData.styledText ?? modelData.text)
                    textFormat: modelData.styledText !== undefined ? Text.StyledText : Text.PlainText
                    wrapMode: Text.WrapAnywhere
                    color: detail.theme.text
                    font { family: detail.theme.font; pixelSize: detail.theme.fontSize }
                }
                Loader {
                    id: emojiLoader
                    anchors.fill: parent
                    active: modelData.kind === "emoji"
                    sourceComponent: Component {
                        TelegramVisual {
                            source: modelData.source || ""
                            format: modelData.format || ""
                            active: detail.visible && !detail.controller.locked && !detail.controller.paused && !detail.historyMode
                            Accessible.name: modelData.text
                        }
                    }
                }
            }
        }
    }
    Text {
        Layout.fillWidth: true
        visible: detail.showBody && detail.notice.body.length > 0 && !detail.mediaData.parts?.length
        objectName: "telegram-message-text"
        text: detail.mediaData.styledText ?? detail.notice.body
        textFormat: detail.mediaData.styledText !== undefined ? Text.StyledText : Text.PlainText
        wrapMode: Text.Wrap
        maximumLineCount: 6
        elide: Text.ElideRight
        color: detail.theme.text
        font { family: detail.theme.font; pixelSize: detail.theme.fontSize }
    }
    Label {
        Layout.fillWidth: true
        visible: text.length > 0
        text: detail.current && detail.playback.error ? detail.playback.error : detail.mediaData.status || ""
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        color: detail.theme.muted
        font { family: detail.theme.font; pixelSize: detail.theme.fontSize - 2 }
    }
    RevealSection {
        id: controlsSection
        Layout.fillWidth: true
        expanded: detail.controlsRevealed && detail.interactive
        Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: detail.theme.border; opacity: 0.6; visible: detail.interactive }
        RowLayout {
            Layout.fillWidth: true
            visible: detail.interactive
            spacing: 12
            IconButton { objectName: "telegram-read";
                iconName: "read"; text: "Mark read"; enabled: detail.controlsRevealed && !command.running; onClicked: detail.action("read", "") }
            IconButton {
                objectName: "telegram-record"
                iconName: "record"
                text: "Record"
                visible: !detail.recordingHere || detail.recorder.phase === "error"
                enabled: detail.controlsRevealed && !command.running && !detail.recorder.holdsNotice
                Accessible.name: detail.mediaData.demo ? "Record a voice note for Saved Messages (@tijptjik)" : "Record a voice reply"

                onClicked: {
                    detail.replying = false;
                    detail.recorder.begin(detail.notice);
                }
            }
            IconButton {
                objectName: "telegram-quote"
                iconName: "quote"
                text: "Quote reply"
                enabled: detail.controlsRevealed && !command.running
                onClicked: detail.compose(true)
            }
            IconButton {
                objectName: "telegram-compose"
                iconName: "reply"
                text: "Reply"
                enabled: detail.controlsRevealed && !command.running
                onClicked: detail.compose(false)
            }
            Item { Layout.fillWidth: true }
            BusyIndicator { implicitWidth: 20; implicitHeight: 20; running: command.running; visible: running }
            IconButton { objectName: "telegram-open";
                iconName: "open"; text: "Open ↗"; enabled: detail.controlsRevealed && !command.running; onClicked: detail.action("open", "") }
        }
    }
    ColumnLayout {
        Layout.fillWidth: true
        visible: detail.recordingHere && detail.interactive
        spacing: 6
        Label {
            Layout.fillWidth: true
            visible: detail.mediaData.demo === true
            text: "Send to Saved Messages · @tijptjik"
            color: detail.theme.muted
            font { family: detail.theme.font; pixelSize: detail.theme.fontSize - 2 }
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 12
            Text {
                Layout.fillWidth: true
                text: (detail.recorder.recording ? "● Recording " : detail.recorder.phase === "sending" ? "Sending voice reply… " : "Voice reply ") + detail.stamp(detail.recorder.duration)
                color: detail.recorder.recording ? detail.theme.accent : detail.theme.muted
                font { family: detail.theme.font; pixelSize: detail.theme.fontSize - 2 }
            }
            IconButton {
                objectName: "telegram-record-discard"
                iconName: "reset"
                text: "Reset recording"
                enabled: detail.controlsRevealed && detail.recorder.phase !== "sending" && detail.recorder.phase !== "discarding"
                onClicked: detail.recorder.discard()
            }
            IconButton {
                objectName: "telegram-record-send"
                iconName: "send"
                text: detail.mediaData.demo ? "Send voice to Saved Messages (@tijptjik)" : "Send voice"
                enabled: detail.controlsRevealed && !detail.recorder.uncertain && (detail.recorder.phase === "ready" ||
                         (detail.recorder.phase === "recording" && detail.recorder.duration > 0))
                onClicked: detail.recorder.submit()
            }
        }
        Text {
            Layout.fillWidth: true
            visible: detail.recorder.error.length > 0
            text: detail.recorder.error
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            color: detail.theme.muted
            font { family: detail.theme.font; pixelSize: detail.theme.fontSize - 2 }
        }
    }
    Text {
        Layout.fillWidth: true
        visible: !detail.recordingHere && !!detail.mediaData.voiceReplyStatus
        text: detail.mediaData.voiceReplyStatus || ""
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        color: detail.theme.muted
        font { family: detail.theme.font; pixelSize: detail.theme.fontSize - 2 }
    }
    ColumnLayout {
        Layout.fillWidth: true
        visible: detail.replying && detail.interactive
        TextArea {
            id: reply
            objectName: "telegram-reply"
            Layout.fillWidth: true
            Layout.preferredHeight: 76
            placeholderText: "Reply…"
            color: detail.theme.text
            placeholderTextColor: detail.theme.muted
            selectionColor: detail.theme.accent
            font { family: detail.theme.font; pixelSize: detail.theme.fontSize - 1 }
            background: Rectangle { color: detail.theme.surface; radius: 6; border.width: 1; border.color: reply.activeFocus ? detail.theme.accent : detail.theme.border }
            wrapMode: TextEdit.Wrap
            selectByMouse: true
            Keys.onEscapePressed: detail.replying = false
            Keys.onPressed: event => {
                if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter) && (event.modifiers & Qt.ControlModifier)) {
                    detail.action("reply", text); event.accepted = true;
                }
            }
        }
        IconButton {
            objectName: "telegram-send"
            Layout.alignment: Qt.AlignRight
            iconName: "send"
            text: "Send reply"
            enabled: detail.controlsRevealed && !command.running && reply.text.trim().length > 0 && reply.text.length <= 4096
            onClicked: detail.action("reply", reply.text)
        }
    }
    Label {
        Layout.fillWidth: true
        visible: detail.feedback.length > 0
        text: detail.feedback
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        color: detail.theme.muted
        font { family: detail.theme.font; pixelSize: detail.theme.fontSize - 2 }
    }
}
