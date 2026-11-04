//@ pragma AppId tasktjik
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
            view.stateData=Object.assign({},view.stateData,state,{tasks:tasks,groups:groups,groupsRevision:view.stateData.groupsRevision || 0});
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
                    const sent = JSON.parse(backend.payload), action = sent.action;
                    if(['toggle','subtask-toggle','subtask-cancel','status'].includes(action) && view.completionQueue[0]?.requestId===sent.request_id)view.acceptCompletion();
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
        id: taskWindow
        // Keep Quickshell's requested visibility in sync with native close events.
        onClosed: visible = false
        title: 'Tasktjik'
        implicitWidth: 860; implicitHeight: 680
        color: '#26233a'
        MouseArea {
            anchors.fill:parent
            enabled:!view.editor && !view.groupRename && !view.dragTask
            onClicked:view.defocusTask()
        }
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
        function reopen(): bool {
            if (!taskWindow.visible) taskWindow.visible = false;
            taskWindow.visible = true;
            taskWindow.minimized = false;
            return true;
        }
        function status(): string {
            return JSON.stringify({windowVisible:taskWindow.visible,windowMinimized:taskWindow.minimized,view:view.view,undatedVisible:view.dropGroups.filter(g=>g.visible).reduce((n,g)=>n+view.tasks(g.modelData).filter(t=>!view.effectiveDue(t)).length,0),project:view.projectId,projectCount:(view.stateData.tasks || []).filter(t=>t.kind==='project').length,visibleCount:view.visibleCount(),visibleEmptyGroups:view.dropGroups.filter(g=>g.visible && g.rows.count===0).length,editor:view.editor, settings:view.settingsOpen, completing:view.completing, saving:!!view.pendingSave, deleting:!!view.pendingPurge, queuedDeletes:view.queuedPurges.length, busy:backend.running || root.pending.length>0, sortOrder:view.sortOrder, dragging:!!view.dragTask, moving:view.pendingMove, purgeMode:'optimistic-undo-queue', taskCount:(view.stateData.tasks || []).length});
        }
        function showView(name: string): bool {
            if(!['today','week','month','all','projects'].includes(name) || view.editor || view.settingsOpen || view.completing || view.pendingSave || view.pendingPurge || view.dragTask || view.pendingMove)return false;
            view.projectId='';view.collection='todo';view.view=name;view.focusList();
            return true;
        }
        function openProject(id: string): bool {
            if(view.editor || view.settingsOpen || view.completing || view.pendingSave || view.pendingPurge || view.dragTask || view.pendingMove || backend.running)return false;
            const project=(view.stateData.tasks || []).find(t=>t.id===id && t.kind==='project');
            if(!project)return false;
            view.collection='todo';view.enterProject(project);return true;
        }
        function projects(): bool {return showView('projects');}
        function reload(): bool {
            if(view.editor || view.settingsOpen || view.completing || view.pendingSave || view.pendingPurge || view.dragTask || view.pendingMove || backend.running || root.pending.length)return false;
            Quickshell.reload(false);
            return true;
        }
    }
    Component.onCompleted: root.request({action:'list'})
}
