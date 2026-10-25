"""Directed synthetic Dunst/Notjik showcase, with optional isolated-output recording."""
import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time

SCENES = ('standard', 'icon', 'status', 'error', 'timer', 'actions-multiple',
          'urgency', 'update', 'grouped', 'telegram-text', 'telegram-photo',
          'telegram-voice', 'telegram-gif', 'robots')
LABELS = dict(zip(SCENES, ('Notifications', 'Sender icons', 'Connection status',
    'Errors and actions', 'Countdown', 'Multiple actions', 'Urgency levels',
    'Progress updates', 'Grouped messages', 'Text replies', 'Photo messages',
    'Voice messages', 'GIF replies', 'Robot artwork')))
CAPTIONS = dict(zip(SCENES, (
    'The same title and message in both notification styles.',
    'Identify the sender from its icon or artwork.',
    'Read the connection state alongside the message.',
    'Hover to reveal the action, then click to inspect the reactor.',
    'Watch the countdown on the card; Dunst shows the original text.',
    'Choose a button on the card; Dunst exposes actions through its menu.',
    'Compare low, normal and critical notifications.',
    'Follow progress as the same notification updates.',
    'Keep related messages in one card; hover to see the conversation.',
    'Type and send a reply from the notification.',
    'View the photo and mark the message as read.',
    'Play the message, then record a short reply.',
    'View the animation and quote it in a reply.',
    'A robot for every feeling')))
ROBOT_SELECTION = (6, 9, 44, 62, 66, 68, 71, 76, 79, 81, 84, 88)


def selected_robots(root):
    from demo import robot_images
    images = {int(p.name.split('-')[1]): p for p in robot_images(root)}
    return [images[number] for number in ROBOT_SELECTION]


def replace(path, old, new):
    source = path.read_text()
    if old not in source:
        raise ValueError('Demo instrumentation needs updating: ' + str(path.name))
    path.write_text(source.replace(old, new, 1))


def prepare(app):
    """Instrument only the private comparison copy; never the installed UI."""
    qml = app / 'qml'
    replace(qml/'shell.qml', 'Variants {\n        model:', 'Variants {\n        id: cyclePanels\n        model:')
    replace(qml/'shell.qml', 'id: root', '''id: root
    property string cycleTitle: "Dunst vs Notjik"
    property string cycleCaption: "Synthetic notification showcase"
    property string cycleCounter: ""
    property bool cycleOutro: false
    property bool cycleIntro: false
    property var cycleIntroCards: []
    DemoStage { controller: root }
''')
    replace(qml/'shell.qml', 'case "status": break;', '''case "cycle": {
            if (request.introCards !== undefined) root.cycleIntroCards = request.introCards;
            if (request.intro !== undefined) root.cycleIntro = request.intro;
            if (request.outro !== undefined) root.cycleOutro = request.outro;
            if (request.title !== undefined) root.cycleTitle = request.title;
            if (request.caption !== undefined) root.cycleCaption = request.caption;
            if (request.counter !== undefined) root.cycleCounter = request.counter;
            const panel = cyclePanels.instances.find(p => p.screen.name === root.config.placement.primaryMonitor);
            if (!panel) throw Error("Demo output not ready");
            if (request.hidePointer) panel.cyclePointerVisible = false;
            if (request.closeDrawer) root.setDrawer(false);
            if (!request.id) return {ok:true};
            const card = panel.cycleCard(request.id);
            if (!card) throw Error("Demo card not ready: " + request.id);
            if (request.target) card.cyclePoint(request.target, !!request.click);
            if (request.text !== undefined) card.cycleDetail.cycleText(request.text);
            const detail = card.cycleDetail;
            return {ok:true, revealed:card.controlsRevealed, hovered:card.hovered, loaded:!!detail, interactive:!!detail?.interactive, replying:!!detail?.replying,
                draft:detail?.cycleDraft || "", feedback:detail?.feedback || "",
                playing:telegramPlayback.playing, recording:telegramRecording.phase,
                duration:telegramRecording.duration};
        }
        case "status": break;''')
    drawer_path = qml/'HistoryDrawer.qml'
    replace(drawer_path, 'implicitWidth: Math.min((theme.width || 420) + 80, (screen?.width || 800) - 32)', 'implicitWidth: 420')
    replace(drawer_path, 'implicitHeight: Math.min(760, (screen?.height || 1000) * 0.72)', 'implicitHeight: 360')
    replace(drawer_path, 'margins { bottom: targetScreen ? targetScreen.height - height - coordinates.y : 16; left: Math.max(0, ((screen?.width || 800) - width) / 2) }', 'margins { bottom: ((screen?.height || 680) - height)/2; left: (screen?.width || 1000)/2 + 20 }')
    panel_path = qml/'NotificationPanel.qml'
    panel_source = panel_path.read_text()
    lines = panel_source.splitlines()
    for i, line in enumerate(lines):
        if 'readonly property var coordinates:' in line:
            lines[i] = '    readonly property var coordinates: ({x: screen.width/2 + 20, y: (screen.height-layoutHeight)/2})'
        elif 'readonly property real layoutHeight:' in line:
            lines[i] = '    readonly property real layoutHeight: 460'
    panel_path.write_text('\n'.join(lines)+'\n')
    replace(panel_path, 'readonly property real stackTop: placement.edge', 'readonly property real stackTop: true ? Math.max(0, (layoutHeight - stack.implicitHeight)/2) : placement.edge')
    replace(qml/'NotificationPanel.qml', 'id: panel', '''id: panel
    property bool cyclePointerVisible: false
    property point cyclePointer: Qt.point(0,0)
    function cycleCard(id) {
        for (let i=0; i<cardItems.count; ++i) {
            const card = cardItems.itemAt(i).cycleCard;
            if (card.notice.id === id) return card;
        }
        return null;
    }
    Rectangle {
        z: 9999; width: 12; height: 12; radius: 6
        visible: panel.cyclePointerVisible
        x: panel.cyclePointer.x - 6; y: panel.cyclePointer.y - 6
        color: "#ffffff"; border.color: "#ff53a6"; border.width: 2
        Behavior on x { NumberAnimation { duration: 160 } }
        Behavior on y { NumberAnimation { duration: 160 } }
    }
''')
    replace(qml/'NotificationPanel.qml', 'property var cachedNotice: null',
            'property var cachedNotice: null\n                    property alias cycleCard: card')
    replace(qml/'NotificationPanel.qml', 'monitorName: panel.screen.name',
            'cycleHost: panel\n                            monitorName: panel.screen.name')
    replace(qml/'NotificationCard.qml', 'import QtQuick\n', 'import QtQuick\nimport QtTest\n')
    replace(qml/'NotificationCard.qml', 'id: card', '''id: card
    TestEvent { id: cycleInput }
    property var cycleHost: null
    property alias cycleDetail: detail.item
    function cyclePoint(name, click) {
        function find(item) {
            if (item.objectName === name && item.visible && item.enabled) return item;
            for (const child of (item.children || [])) { const match=find(child); if(match) return match; }
            return null;
        }
        const item = name === "card" ? card : name === "group" ? groupIndicator : find(card);
        if (!item) throw Error("Demo control unavailable: " + name);
        const x = item.width/2, y = name === "card" ? 20 : item.height/2;
        cycleHost.cyclePointer = item.mapToItem(cycleHost.contentItem, x, y);
        cycleHost.cyclePointerVisible = true;
        cycleInput.mouseMove(item, x, y, 0, Qt.NoButton, Qt.NoModifier);
        if (click) cycleInput.mouseClick(item, x, y, Qt.LeftButton, Qt.NoModifier, 0);
    }
''')
    replace(qml/'NotificationCard.qml', 'id: actionButton', 'id: actionButton\n                                        objectName: "cycle-action-" + modelData.actionIndex')
    replace(qml/'components/Telegram.qml', 'id: detail', '''id: detail
    readonly property alias cycleDraft: reply.text
    function cycleText(text) { reply.text = text; }
''')
    replace(qml/'components/Telegram.qml', 'Math.min(240, width *', 'Math.min(150, width *')
    # Recording simulation cannot open the microphone or contact the real bridge.
    replace(qml/'TelegramRecorder.qml', 'function begin(notice) {', '''function begin(notice) {
        if (!notice.data.demo) throw Error("Only synthetic demo recordings are allowed");
        controller.telegramPlayer.stop(); noticeId = notice.id;
        phase = "recording"; duration = 0; source = ""; error = ""; uncertain = false;
        return;
''')
    replace(qml/'TelegramRecorder.qml', 'function submit() {', '''function submit() {
        phase = "sending"; cycleSend.restart(); return;
''')
    replace(qml/'TelegramRecorder.qml', 'id: recorder', '''id: recorder
    Timer { interval: 100; repeat: true; running: recorder.phase === "recording"; onTriggered: recorder.duration += 100 }
    Timer { id: cycleSend; interval: 450; onTriggered: {
        const id = recorder.noticeId; recorder.reset(); controller.close(id, false);
    } }
''')
    # A second boundary: even accidental helper invocation cannot send or capture.
    for helper in ('telegram_bridge.py', 'telegram_recording.py'):
        (app/helper).write_text('raise SystemExit("Real Telegram and microphone access are disabled in the demo cycle")\n')
    path = qml/'components/Telegram.qml'
    path.write_text(path.read_text().replace('Saved Messages (@tijptjik)', 'demo chat (simulated)'))
    replace(qml/'TelegramPlayback.qml', 'AudioOutput {}', 'AudioOutput { muted: true }')
    (qml/'DemoStage.qml').write_text(STAGE)


STAGE = '''import QtQuick
import Quickshell
import Quickshell.Wayland
PanelWindow {
    id: stage
    property real comparisonReveal: controller.cycleIntro ? 0 : 1
    Behavior on comparisonReveal { NumberAnimation { duration: 450; easing.type: Easing.InOutQuad } }
    required property var controller
    screen: Quickshell.screens.find(s => s.name === controller.config.placement.primaryMonitor) || Quickshell.screens[0]
    visible: controller.ready
    implicitWidth: 1000
    implicitHeight: 680
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.namespace: "notjik-demo-stage"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    mask: Region { width: 0; height: 0 }
    color: "#15131f"
    Text { x: 48; y: 20; textFormat: Text.StyledText; text: "Dunst <font color='#ffffff'>vs</font> Notjik"; color: "#ff53a6"; font { family: "tijpset"; pixelSize: 16; bold: true } }
    Text { anchors.right: parent.right; anchors.rightMargin: 48; y: 20; text: stage.controller.cycleCounter; color: "#a5a0ba"; font { family: "tijpset"; pixelSize: 16 } }
    Column {
        opacity: stage.controller.cycleIntro || stage.controller.cycleOutro ? 1 : stage.comparisonReveal
        y: stage.controller.cycleOutro ? parent.height/2 - 35 : 55; width: parent.width; spacing: 12
        Text { anchors.horizontalCenter: parent.horizontalCenter; text: stage.controller.cycleTitle; color: "#e5e5e5"; font { family: "tijpset"; pixelSize: 25; bold: true } }
        Text { anchors.horizontalCenter: parent.horizontalCenter; text: stage.controller.cycleCaption; color: "#a5a0ba"; font { family: "tijpset"; pixelSize: 14 } }
    }
    Text { opacity: stage.comparisonReveal; visible: !stage.controller.cycleOutro && opacity > 0; x: parent.width/2 - 230 - width/2; y: 140; text: "DUNST"; color: "#a5a0ba"; font { family: "tijpset"; pixelSize: 18; bold: true } }
    Text { opacity: stage.comparisonReveal; visible: !stage.controller.cycleOutro && opacity > 0; x: parent.width/2 + 230 - width/2; y: 140; text: "NOTJIK"; color: "#ff53a6"; font { family: "tijpset"; pixelSize: 18; bold: true } }
    Item {
        id: montage
        x: 48; y: 145; width: 904; height: 470
        opacity: stage.controller.cycleIntro ? 1 : 0
        scale: stage.controller.cycleIntro ? 1 : 0.96
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: 800; easing.type: Easing.InOutQuad } }
        Behavior on scale { NumberAnimation { duration: 800; easing.type: Easing.InOutQuad } }
        Row {
            spacing: 24
            Repeater {
                model: 4
                Column {
                    required property int index
                    width: 208; spacing: 15
                    Repeater {
                        model: stage.controller.cycleIntroCards.filter((item, i) => i % 4 === index)
                        Item {
                            required property var modelData
                            property real reveal: 0
                            opacity: reveal
                            scale: 0.82 + 0.18 * reveal
                            rotation: (1-reveal) * (modelData.demoOrder % 2 ? 4 : -4)
                            transform: Translate { y: (1-reveal) * 38 }
                            Behavior on reveal { NumberAnimation { duration: 480; easing.type: Easing.OutCubic } }
                            Timer { interval: 50 + modelData.demoOrder * 65; running: true; onTriggered: parent.reveal = 1 }
                            width: 208; height: preview.implicitHeight * 0.495 + 22
                            Text { text: modelData.demoLabel; color: "#a5a0ba"; font { family: "tijpset"; pixelSize: 12 } }
                            NotificationCard {
                                id: preview
                                y: 22; width: 420; height: implicitHeight
                                scale: 0.495; transformOrigin: Item.TopLeft
                                notice: modelData
                                controller: stage.controller
                                monitorName: stage.screen.name
                                enabled: false
                            }
                        }
                    }
                }
            }
        }
    }
    Text { anchors.horizontalCenter: parent.horizontalCenter; anchors.bottom: parent.bottom; anchors.bottomMargin: 18;
        textFormat: Text.StyledText;
        text: "Design by <font color='#ff53a6'>TIJPTJIK</font> · Implementation by <font color='#ff53a6'>ASTRA</font>";
        color: "#777185"; font { family: "tijpset"; pixelSize: 13 } }
}
'''


def checked(args, **kwargs):
    return subprocess.check_output(args, text=True, stderr=subprocess.PIPE, **kwargs).strip()


@contextmanager
def output_monitor(record, selected=None):
    if selected:
        monitors = json.loads(checked(['hyprctl', '-j', 'monitors']))
        if selected not in {m['name'] for m in monitors}:
            raise ValueError('Unknown output: ' + selected)
        yield selected
        return
    if not record:
        monitors = json.loads(checked(['hyprctl', '-j', 'monitors']))
        yield next((m['name'] for m in monitors if m.get('focused')), monitors[0]['name'])
        return
    before = {m['name'] for m in json.loads(checked(['hyprctl','-j','monitors']))}
    name = None
    try:
        checked(['hyprctl', 'output', 'create', 'headless'])
        for _ in range(30):
            new = [m for m in json.loads(checked(['hyprctl','-j','monitors'])) if m['name'] not in before]
            if new:
                if len(new) != 1: raise ValueError('Ambiguous new recording output')
                name = new[0]['name']; break
            time.sleep(.1)
        if name is None: raise ValueError('Headless recording output did not appear')
        result = checked(['hyprctl', 'eval', 'hl.monitor({output='+json.dumps(name)+',mode="1000x680@60",position="10000x10000",scale=1})'])
        if result != 'ok': raise ValueError('Cannot configure recording output: '+result)
        yield name
    finally:
        if name:
            subprocess.run(['hyprctl','output','remove',name], capture_output=True, timeout=5)


def wait_for(predicate, timeout=8):
    deadline = time.monotonic() + timeout
    last_error = None
    while time.monotonic() < deadline:
        try:
            value = predicate()
            if value: return value
        except ValueError as error:
            last_error = error
        time.sleep(.08)
    raise ValueError('Timed out waiting for demo UI: ' + str(last_error))


class Cycle:
    def __init__(self, root, session, speed=1):
        from demo import PairedDemo
        self.root, self.session, self.speed = root, session, speed
        self.paired = PairedDemo(session)
        self.timeline = []
        self.started = time.monotonic()

    def pause(self, seconds):
        time.sleep(max(.12, seconds * self.speed))

    def control(self, **data):
        from notjik import ipc
        return ipc(Path(self.session['app']), {'op':'cycle', **data})

    def caption(self, text):
        self.control(caption=text)

    def point(self, ident, target, click=False):
        try:
            return self.control(id=ident, target=target, click=click)
        except ValueError:
            print('Demo control state: '+json.dumps(self.control(id=ident)), file=sys.stderr)
            raise

    def type_reply(self, ident, quote=False):
        self.point(ident, 'telegram-quote' if quote else 'telegram-compose', True)
        self.pause(.5)
        text = 'Our new captain?' if quote else 'On my way. Bringing a scanner.'
        for end in range(3, len(text)+3, 3):
            self.control(id=ident, text=text[:end]); self.pause(.12)
        self.pause(.4)
        self.point(ident, 'telegram-send'); self.pause(.3)
        self.point(ident, 'telegram-send', True)
        self.pause(.5)

    def scene(self, name, index, total):
        import comparison
        from demo import APP, fixtures, reference_note, show_robots
        self.paired.clear()
        self.control(hidePointer=True, intro=False, title=LABELS[name], caption=CAPTIONS[name], counter=f'{index:02d} / {total:02d}')
        self.pause(.85 if index == 1 else .55)
        if index == 1:
            self.control(introCards=[])
        comparison.set_grouping(self.session, name == 'grouped')
        self.caption(CAPTIONS[name])
        start = time.monotonic()-self.started
        if name == 'robots':
            for image in selected_robots(self.root):
                self.paired({'op':'send','notice':{'app':APP,'id':'demo-robots','group':'robots',
                    'title':image.stem[9:].replace('-', ' ').capitalize(),
                    'body':'Shipboard companion unit', 'image':image.as_uri(), 'data':{'demoRobot':True},
                    'urgency':'critical' if '-alert-' in image.stem else 'low' if '-dream-' in image.stem else 'normal',
                    'timeout':0,'transient':True}})
                time.sleep(.25)
            self.pause(1)
            self.paired.clear()
            self.control(closeDrawer=True)
            time.sleep(max(.7, .7 * self.speed))
            self.timeline.append({'scene':name,'start':round(start,3),'end':round(time.monotonic()-self.started,3)})
            print(f'{index}/{total} {name}',flush=True)
            return
        ids=[]; notices=[]
        for n, fixture in enumerate(fixtures(self.root,name)):
            notice={'app':APP,'id':f'cycle-{name}-{n}', 'group':name if name=='grouped' else f'{name}-{n}',
                    'timeout':0, 'transient':True, **fixture}
            if name=='timer': notice['data']={**notice['data'],'seconds':3}
            ident=self.paired({'op':'send','notice':notice})['id']
            ids.append(ident); notices.append(notice)
            if name in ('urgency','grouped'): self.pause(.55)
        ident=ids[-1]
        wait_for(lambda:self.control(id=ident))
        self.pause(1)
        if name in ('error','actions-multiple') or name.startswith('telegram-') or name=='grouped':
            self.point(ident,'card'); self.pause(.55)
        if name=='error':
            self.caption(CAPTIONS[name])
            self.point(ident,'cycle-action-0'); self.pause(.7)
            self.point(ident,'cycle-action-0',True); self.pause(.5)
        elif name=='actions-multiple':
            self.caption(CAPTIONS[name])
            for target in ('cycle-action-1','cycle-action-0'):
                self.point(ident,target); self.pause(.85)
        elif name=='timer': self.pause(2.5)
        elif name=='update':
            for value in (30,60,85,100):
                notice={**notices[0],'data':{'value':value,'label':'Sector mapped' if value==100 else 'Receiving star charts'}}
                assert self.paired({'op':'send','notice':notice})['id']==ident
                self.pause(.6)
        elif name=='grouped':
            self.caption(CAPTIONS[name])
            self.point(ident,'group')
            from notjik import ipc
            wait_for(lambda: (lambda state: state.get('drawerOpen') and state.get('historyCount', 0) >= 3)(ipc(Path(self.session['app']), {'op':'status'})))
            self.pause(2)
        elif name=='telegram-text':
            self.caption(CAPTIONS[name]); self.type_reply(ident)
        elif name=='telegram-photo':
            self.caption(CAPTIONS[name])
            self.point(ident,'telegram-read'); self.pause(.5)
            state=self.point(ident,'telegram-read',True)
            self.pause(.9)
        elif name=='telegram-voice':
            self.caption(CAPTIONS[name])
            self.point(ident,'telegram-play',True); self.pause(2)
            self.point(ident,'telegram-play',True); self.pause(.2)
            self.point(ident,'telegram-record',True)
            wait_for(lambda:self.control(id=ident)['recording']=='recording')
            self.pause(3.2)
            self.point(ident,'telegram-record-send'); self.pause(.3)
            self.point(ident,'telegram-record-send',True); self.pause(.7)
        elif name=='telegram-gif':
            self.caption(CAPTIONS[name]); self.type_reply(ident,quote=True)
        else: self.pause(1)
        self.control(hidePointer=True)
        self.paired.clear()
        self.control(closeDrawer=True)
        # Let the actual removal animation finish before the next intro.
        time.sleep(max(.7, .7 * self.speed))
        self.timeline.append({'scene':name,'start':round(start,3),'end':round(time.monotonic()-self.started,3)})
        print(f'{index}/{total} {name}',flush=True)

    def introduction(self):
        from demo import APP, fixtures
        cards = []
        images = selected_robots(self.root)
        for name in SCENES:
            if name == 'robots':
                continue
            for fixture in (fixtures(self.root, name) if name == 'urgency' else fixtures(self.root, name)[:1]):
                i = len(cards)
                notice = {'id':f'intro-{i}', 'app':APP, 'type':'standard', 'body':'',
                    'urgency':'normal', 'actions':[], 'data':{}, 'icon':'', **fixture,
                    'image':images[i % len(images)].as_uri(), 'received':int(time.time()*1000),
                    'remaining':0, 'generation':i, 'groupCount':3 if name == 'grouped' else 1,
                    'demoOrder':i, 'demoLabel':LABELS[name] + (' · ' + fixture['urgency'] if name == 'urgency' else '')}
                cards.append(notice)
        self.control(title='Notjik', caption='A robot for every feeling', counter='', introCards=cards, intro=True)
        self.pause(2)

    def run(self, scenes):
        self.introduction()
        for index,name in enumerate(scenes,1): self.scene(name,index,len(scenes))
        self.control(title='github.com/tijptjik/tooltjik',caption='Source · Installation · Documentation',counter='',outro=True)
        self.pause(3)


def main(root, argv):
    parser=argparse.ArgumentParser(prog='notjikctl demo',description=__doc__)
    parser.add_argument('mode',choices=('cycle','record'))
    parser.add_argument('output',nargs='?',type=Path)
    parser.add_argument('--monitor', help='present and record on this existing Wayland output')
    parser.add_argument('--only',nargs='+',choices=SCENES,help='run selected scenes in the specified order')
    parser.add_argument('--speed',type=float,default=1,help='duration multiplier (default 1)')
    parser.add_argument('--plan',action='store_true',help='print scene order without opening UI')
    args=parser.parse_args(argv)
    if not .1 <= args.speed <= 4: parser.error('--speed must be between 0.1 and 4')
    if args.mode=='record' and not args.output: parser.error('record requires OUTPUT.mp4')
    if args.mode=='cycle' and args.output: parser.error('cycle takes no output path')
    scenes=args.only or SCENES
    if args.plan:
        print('\n'.join(f'{i}. {name} — {LABELS[name]}' for i,name in enumerate(scenes,1))); return 0
    output=args.output.expanduser().absolute() if args.output else None
    if output and (output.suffix.lower()!='.mp4' or output.exists() or output.with_suffix('.chapters.json').exists()):
        parser.error('choose a new .mp4 output file; existing files are never overwritten')
    for command in ('hyprctl','quickshell','busctl','dbus-daemon',*(('wf-recorder','ffprobe','ffmpeg') if output else ())):
        if not shutil.which(command): parser.error('required command missing: '+command)
    import comparison
    recorder=None; partial=None; log=None
    previous={key:os.environ.get(key) for key in ('NOTJIK_DEMO_CYCLE_DIR','NOTJIK_DEMO_CYCLE_MONITOR')}
    try:
        with tempfile.TemporaryDirectory(prefix='notjik-cycle-') as temporary, output_monitor(bool(output), args.monitor) as monitor:
            os.environ['NOTJIK_DEMO_CYCLE_DIR']=temporary
            os.environ['NOTJIK_DEMO_CYCLE_MONITOR']=monitor
            session=comparison.start(root,None)
            try:
                cycle=Cycle(root,session,args.speed)
                if output:
                    # Let the compositor's layer fade finish before capturing any frames.
                    cycle.control(title='Notjik', caption='A robot for every feeling', counter='', intro=True)
                    time.sleep(2.5)
                    output.parent.mkdir(parents=True,exist_ok=True)
                    handle=tempfile.NamedTemporaryFile(prefix='.'+output.stem+'-',suffix='.mp4',dir=output.parent,delete=False)
                    partial=Path(handle.name); handle.close(); partial.unlink()
                    log=open(Path(temporary)/'recorder.log','w')
                    geometry=[]
                    if args.monitor:
                        info=next(m for m in json.loads(checked(['hyprctl','-j','monitors'])) if m['name']==monitor)
                        x=round(info['x']+(info['width']/info['scale']-1000)/2)
                        y=round(info['y']+(info['height']/info['scale']-680)/2)
                        geometry=['-g',f'{x},{y} 1000x680']
                    cycle.started = time.monotonic()
                    recorder=subprocess.Popen(['wf-recorder','-D','--no-dmabuf','-o',monitor,'-r','60','-c','libx264rgb','-p','preset=ultrafast','-p','crf=0','-x','bgr0',*geometry,'-f',str(partial)],stdout=log,stderr=log)
                    time.sleep(.1)
                    if recorder.poll() is not None: raise ValueError('Recorder failed to start: '+(Path(temporary)/'recorder.log').read_text())
                cycle.run(scenes)
                if recorder:
                    recorder.send_signal(signal.SIGINT); recorder.wait(timeout=15); recorder=None; log.close()
                    checked(['ffprobe','-v','error','-show_entries','format=duration','-of','default=nw=1:nk=1',str(partial)])
                    playable=Path(temporary)/'playable.mp4'
                    checked(['ffmpeg','-v','error','-i',str(partial),'-c:v','libx264','-preset','slow','-crf','14','-pix_fmt','yuv420p','-profile:v','high','-movflags','+faststart',str(playable)])
                    shutil.copyfile(playable,partial)
                    os.link(partial,output); partial.unlink(); partial=None
                    with output.with_suffix('.chapters.json').open('x') as manifest:
                        json.dump({'synthetic':True,'scenes':cycle.timeline},manifest,indent=2)
                    print('Recording: '+str(output))
            finally:
                if recorder:
                    recorder.send_signal(signal.SIGINT)
                    try: recorder.wait(timeout=5)
                    except subprocess.TimeoutExpired: recorder.kill(); recorder.wait()
                comparison.stop()
                if sys.exc_info()[0]:
                    diagnostic = Path(temporary)/"session.log"
                    if diagnostic.exists(): print(diagnostic.read_text()[-5000:], file=sys.stderr)
        return 0
    except KeyboardInterrupt:
        print('Demo cancelled; private comparison session closed.',file=sys.stderr); return 130
    except (OSError,ValueError,subprocess.SubprocessError) as error:
        print('notjik demo: '+str(error),file=sys.stderr); return 1
    finally:
        if log: log.close()
        if partial: partial.unlink(missing_ok=True)
        for key,value in previous.items():
            if value is None: os.environ.pop(key,None)
            else: os.environ[key]=value
