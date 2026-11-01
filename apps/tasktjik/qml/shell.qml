import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io

ShellRoot {
    id: root
    property var pending: []
    function applyState(state) {
        if(state.incremental) {
            const changed=new Set((state.tasks || []).map(t=>t.id).concat(state.removed || []));
            const tasks=(view.stateData.tasks || []).filter(t=>!changed.has(t.id)).concat(state.tasks || []);
            const groups=Array.from(new Set((view.stateData.groups || []).concat(state.groups || [])));
            view.stateData=Object.assign({},view.stateData,state,{tasks:tasks,groups:groups});
        } else view.stateData=Object.assign({},view.stateData,state);
    }
    function request(value) {
        pending.push(value);
        drain();
    }
    function drain() {
        if (backend.running || !pending.length) return;
        backend.payload = JSON.stringify(pending.shift());
        backend.output = '';
        backend.failure = '';
        view.errorMessage = '';
        backend.running = true;
    }
    Process {
        id: backend
        property string payload
        property string output
        property string failure
        command: [Quickshell.env('TASKTJIK_HOME') + '/tasktjik-api', 'state', '--incremental'].concat(Quickshell.env('TASKTJIK_DEMO') === '1' ? ['--demo'] : [])
        stdinEnabled: true
        onStarted: { write(payload); stdinEnabled = false; }
        stdout: StdioCollector { onStreamFinished: backend.output = text }
        stderr: StdioCollector { onStreamFinished: backend.failure = text.trim() }
        onExited: (exitCode, exitStatus) => {
            if(exitCode===0) {
                try {
                    root.applyState(JSON.parse(output));
                    view.errorMessage = '';
                    // Reconcile the optimistic row only after the server confirms it.
                    const action = JSON.parse(backend.payload).action;
                    if (['create', 'edit', 'snooze'].includes(action)) {
                        view.accepted();
                        Qt.callLater(() => view.forceActiveFocus());
                    }
                }
                catch (e) { view.errorMessage = 'Could not load tasks. Check Tasktjik authority configuration.'; }
            } else view.errorMessage = failure || 'Could not save or load tasks. Please try again.';
            stdinEnabled = true;
            Qt.callLater(root.drain);
        }
    }
    FloatingWindow {
        title: 'Tasktjik'
        implicitWidth: 860; implicitHeight: 680
        color: '#26233a'
        TodoApp {
            id: view
            anchors.fill: parent; anchors.margins: 20
            focus: true
            requestBusy: backend.running || root.pending.length>0
            view: Quickshell.env('TASKTJIK_VIEW') || 'all'
            collection: Quickshell.env('TASKTJIK_COLLECTION') || 'todo'
            selectedId: Quickshell.env('TASKTJIK_SELECTED') || ''
            theme: ({background:'#26233a',surface:'#39354b',text:'#f5f5f7',muted:'#aaa5ba',accent:'#a8c7a0',border:'#555169'})
            onRequest: value => root.request(value)
            onExitApp: Qt.quit()
        }
    }
    Timer { interval: 10000; running: true; repeat: true; onTriggered: { if (!view.editor && !backend.running) root.request({action:'list'}); } }
    IpcHandler {
        target: "tasktjik"
        function status(): string {
            return JSON.stringify({editor:view.editor, saving:!!view.pendingSave, deleting:!!view.pendingPurge, queuedDeletes:view.queuedPurges.length, busy:backend.running || root.pending.length>0, sortOrder:view.sortOrder, dragging:!!view.dragTask, moving:view.pendingMove, purgeMode:'optimistic-undo-queue', taskCount:(view.stateData.tasks || []).length});
        }
        function reload(): bool {
            if(view.editor || view.pendingSave || view.pendingPurge || view.dragTask || view.pendingMove || backend.running || root.pending.length)return false;
            Quickshell.reload(false);
            return true;
        }
    }
    Component.onCompleted: root.request({action:'list'})
}
