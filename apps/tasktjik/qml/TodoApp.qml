import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import QtCore
import Quickshell
import Quickshell.Io
import "DueDate.js" as DueDate
import "Repeat.js" as Repeat
import "DateCode.js" as DateCode
import "Workflow.js" as Workflow

Pane {
    padding: 0
    background: Rectangle {color:"transparent"}
    palette.windowText: theme.text
    palette.window: theme.background
    palette.base: theme.background
    palette.button: theme.surface
    palette.text: theme.text
    palette.buttonText: theme.text
    palette.highlight: theme.accent
    palette.highlightedText: theme.background
    id: app
    component EdgeScrollBar: ScrollBar {
        required property var scrollView
        // Keep the viewport padded, but place its attached bar at the panel edge.
        parent: app.parent
        x: parent.width-width
        y: scrollView.y+scrollView.parent.mapToItem(parent,0,0).y
        height: scrollView.height
        padding: 0
        z: 20
        palette: app.palette
        visible: app.visible && scrollView.visible && size<1
        enabled: scrollView.enabled
    }
    property var stateData: ({tasks:[],groups:[]})
    property var theme
    property string collection: 'todo'
    property string projectId: ''
    property string projectReturnView: 'all'
    property string previousOverviewView: 'week'
    readonly property string newKind: collection==='reading'?'reading':collection==='template'?'template':view==='projects' && !projectId?'project':'todo'
    function newDefault() {newItem(newKind);}
    function leaveProject() {projectId='';view=projectReturnView;selectVisibleTask();}
    property var expandedTasks: ({})
    readonly property var currentProject: (listState?.tasks || []).find(t=>t.id===projectId) || null
    function enterProject(task) {projectReturnView=view;groupColumns.placements={};groupColumns.measuredHeights={};projectId=task.id;view='all';filter='';selectedId='';groupColumns.balance();taskScroll.contentItem.contentY=0;selectVisibleTask();}
    function newItem(kind,group) {selectedId='';openEditor({title:'',kind:kind,project:kind==='todo'?projectId:'',group:group || (currentProject?currentProject.group:'All'),tags:[],due:'',repeat:{},reference:'',subtasks:[]});}
    function socialTitle(task) {return String(task.group || '').toLowerCase()==='socials'?task.title.replace(/^(Meet|Message)\s+(?=\S)/i,''):task.title;}
    function socialTags(task) {
        if(String(task.group || '').toLowerCase()!=='socials')return [];
        const tags=(task.tags || []).map(tag=>tag.replace(/^#/,'').toLowerCase());
        return ['message','hangout','call'].filter(tag=>tags.includes(tag));
    }
    function addSubtask(task) {selectedId=task.id;openEditor();workflowEditor.add('task','');}
    function projectProgress(task) {
        let rows=(listState?.tasks || []).filter(t=>t.project===task.id && t.kind==='todo' && t.status!=='cancelled');
        if(!['today','week','month'].includes(view))return {total:rows.length,done:rows.filter(t=>t.done).length};
        const today=listState.today || Qt.formatDate(new Date(),'yyyy-MM-dd');
        const start=new Date(today+'T12:00:00'),end=new Date(start);
        if(view==='week'){start.setDate(start.getDate()-(start.getDay()+6)%7);end.setTime(start.getTime());end.setDate(end.getDate()+6);}
        if(view==='month'){start.setDate(1);end.setMonth(end.getMonth()+1,0);}
        const first=Qt.formatDate(start,'yyyy-MM-dd'),last=Qt.formatDate(end,'yyyy-MM-dd');
        const within=d=>!!d && d>=first && d<=last;
        const completed=t=>within(String(t.completedAt || '').slice(0,10)) || Object.values(t.completions || {}).some(d=>within(String(d).slice(0,10)));
        rows=rows.filter(t=>within(effectiveDue(t)) || completed(t));
        return {total:rows.length,done:rows.filter(completed).length};
    }
    function toggleExpanded(id) {const next=Object.assign({},expandedTasks);next[id]=!next[id];expandedTasks=next;}

    property string draftId: ''
    property string lastDraftId: ''
    property string view: 'week'
    property bool searching: false
    property bool showCompleted: false
    property bool showCancelled: false
    property bool showBlocked: false
    property string selectedSubtaskId: ''
    property bool showDetails: false
    property bool readingGrid: false
    property bool readingCategories: false
    property int readingSort: 0
    property int sortOrder: 0
    property alias doubleDInterval: viewPreferences.doubleDInterval
    property alias domainSort:viewPreferences.domainSort
    property alias manualDomainOrderJson:viewPreferences.manualDomainOrderJson
    readonly property var manualDomainOrder:JSON.parse(manualDomainOrderJson || '[]')
    property var collapsedDomains:({})
    function toggleDomain(group) {
        collapsedDomains=Object.assign({},collapsedDomains,{[group]:!collapsedDomains[group]});
        if(collapsedDomains[group] && selected && selected.group===group){taskSelectionActive=false;selectedSubtaskId='';}
    }
    function moveDomain(group,direction) {
        const order=manualGroups(),index=order.indexOf(group),to=index+direction;
        if(index<0 || to<0 || to>=order.length)return;
        order.splice(index,1);order.splice(to,0,group);manualDomainOrderJson=JSON.stringify(order);domainSort='manual';groupColumns.reset();
    }
    function manualGroups() {
        const available=(listState?.groups || []);
        return manualDomainOrder.filter(g=>available.includes(g)).concat(available.filter(g=>!manualDomainOrder.includes(g)));
    }
    onDomainSortChanged:groupColumns.reset()
    property alias projectSort: viewPreferences.projectSort
    Settings {
        id:viewPreferences
        location:'file://'+(Quickshell.env('XDG_CONFIG_HOME') || Quickshell.env('HOME')+'/.config')+'/tasktjik/ui.ini'
        category:'tasks'
        property string projectSort:'first-due'
        property string domainSort:'manual'
        property string manualDomainOrderJson:'[]'
        property int doubleDInterval:350
    }
    onProjectSortChanged: {if(dragTask && !pendingMove)cancelDrag();selectVisibleTask();}
    property bool requestBusy: false
    property var completionQueue: []
    property string completionMessage: ''
    readonly property bool completing: completionQueue.length>0
    function cancelFocusedTask() {
        if(!taskSelectionActive || !selected || selected.kind!=='todo' || selected.status==='cancelled' || pendingSave || pendingPurge || pendingMove || dragTask)return;
        if(selectedSubtaskId){
            if(!Workflow.cancel(selected,selectedSubtaskId,stateData.today))return;
            completionQueue=completionQueue.concat([{id:selected.id,cancelStep:true,subtaskId:selectedSubtaskId,requestId:newRequestId(),sent:false}]);
            sendCompletion();return;
        }
        completionMessage='';
        completionQueue=completionQueue.concat([{id:selected.id,status:'cancelled',subtaskId:'',requestId:newRequestId(),sent:false}]);
        Qt.callLater(()=>{selectVisibleTask();focusList();});
        sendCompletion();
    }
    function projectCompletion(task,operation,today) {
        if(operation.cancelStep)return Workflow.cancel(task,operation.subtaskId,today);
        if(operation.status)return Object.assign({},Workflow.project(task,'',false,today),{status:operation.status,blockers:[]});
        return Workflow.project(task,operation.subtaskId,operation.done,today);
    }
    function toggleCompletion(taskId,subtaskId) {
        if(pendingSave || pendingPurge || pendingMove || dragTask)return;
        const task=(listState.tasks || []).find(t=>t.id===taskId);
        if(!task)return;
        const target=subtaskId?Workflow.workflowNode(task.subtasks,subtaskId):task;
        if(!target)return;
        const operation={id:taskId,subtaskId:subtaskId || '',done:!target.done,requestId:newRequestId(),sent:false};
        if(!Workflow.project(task,operation.subtaskId,operation.done,stateData.today))return;
        completionMessage='';completionQueue=completionQueue.concat([operation]);
        if(!subtaskId)Qt.callLater(()=>{selectVisibleTask();focusList();});
        sendCompletion();
    }
    function sendCompletion() {
        if(!completionQueue.length || completionQueue[0].sent || requestBusy)return;
        const operation=completionQueue[0],task=(stateData.tasks || []).find(t=>t.id===operation.id);
        const target=task && (operation.subtaskId?Workflow.workflowNode(task.subtasks,operation.subtaskId):task);
        if(!target || !projectCompletion(task,operation,stateData.today)) {rejectCompletion('This step changed; review its current state.');return;}
        if(!operation.cancelStep && (operation.status?target.status===operation.status:target.done===operation.done)){completionQueue=completionQueue.slice(1);return;}
        completionQueue=[Object.assign({},operation,{sent:true})].concat(completionQueue.slice(1));
        request({action:operation.cancelStep?'subtask-cancel':operation.status?'status':operation.subtaskId?'subtask-toggle':'toggle',status:operation.status,actor:'user',id:task.id,subtask_id:operation.subtaskId,period:task.period,revision:task.revision,request_id:operation.requestId});
    }
    function acceptCompletion() {
        if(completionQueue.length && completionQueue[0].sent)completionQueue=completionQueue.slice(1);
    }
    function rejectCompletion(message) {
        if(!completionQueue.length)return;
        const failed=completionQueue[0];
        // Later changes to this task may depend on the failed step; independent tasks continue.
        completionQueue=completionQueue.filter(op=>op.id!==failed.id);
        completionMessage=message || errorMessage || 'Could not save the task change. Your change was restored.';
    }
    Timer {interval:25;repeat:true;running:app.completing;onTriggered:app.sendCompletion()}
    property var pendingSave: null
    property var retrySave: null
    property var dueToday: new Date()
    readonly property var parsedDue: DueDate.parse(dueInput.text, dueToday)
    readonly property var dueSuggestions: DueDate.suggestions(dueInput.text, dueToday)
    readonly property var parsedRepeat: Repeat.parse(repeatInput.text)
    property bool repeatTyping: true
    property int dueSuggestionIndex: -1
    property bool dueSuggestionsDismissed: false
    readonly property bool showDueSuggestions: dueInput.activeFocus && !dueSuggestionsDismissed && dueSuggestions.length>0
    function acceptDueSuggestion(index) {
        if(index<0 || index>=dueSuggestions.length)return;
        dueInput.text=dueSuggestions[index].value;
        dueSuggestionsDismissed=true;dueSuggestionIndex=-1;
        dueInput.forceActiveFocus();
    }
    function savingState() {
        const task=Object.assign({done:false,status:'ready',period:'once',completions:{}},pendingSave.draft,
            {id:pendingSave.request.id || pendingSave.request.request_id,saving:true});
        const rows=(stateData.tasks || []).filter(t=>t.id!==task.id).concat([task]);
        const groups=(stateData.groups || []).slice();
        if(!groups.includes(task.group))groups.push(task.group);
        return Object.assign({},stateData,{tasks:rows,groups:groups});
    }
    function projectedState() {
        const state=pendingSave && !pendingSave.request.draft_id ? savingState() : stateData;
        let tasks=(state.tasks || []).slice();
        if(pendingMove && moveOperation)tasks=tasks.map(task=>task.id===moveOperation.id?
            Object.assign({},task,{group:moveOperation.group},moveOperation.project!==undefined?{project:moveOperation.project}:{},moveOperation.group==='Quick'?{due:state.today || Qt.formatDate(new Date(),'yyyy-MM-dd')}:{}) : task);
        for(const operation of (pendingPurge?[pendingPurge]:[]).concat(queuedPurges)) {
            if(operation.undoRequested) {
                if(!tasks.some(t=>t.id===operation.id))tasks.push(operation.task);
            } else tasks=tasks.filter(t=>t.id!==operation.id);
        }
        for(const operation of completionQueue)tasks=tasks.map(task=>task.id===operation.id?
            (projectCompletion(task,operation,state.today) || task):task);
        return Object.assign({},state,{tasks:tasks});
    }
    property var dragTask: null
    property var dragSnapshot: ({})
    readonly property var listState: dragTask ? dragSnapshot : projectedState()
    property var dragSource: null
    property point dragPoint: Qt.point(0,0)
    property real dragWidth: 300
    property real dragHeight: 40
    property var dropGroups: []
    property string dropGroup: ''
    property string dropBefore: ''
    property string dropProject:''
    property bool pendingMove: false
    property bool moveSent: false
    property var moveOperation: null
    property var queuedMoves: []
    property string moveMessage: ''
    onSortOrderChanged: {if(dragTask && !pendingMove)cancelDrag();}
    function cancelDrag() {
        dropGroup='';dropBefore='';dropProject='';dragTask=null;dragSource=null;
    }
    function clearMove() {
        moveTimeout.stop();pendingMove=false;moveSent=false;moveOperation=null;queuedMoves=[];
    }
    function acceptMove() {
        if(!queuedMoves.length){clearMove();return;}
        const next=queuedMoves[0];
        moveOperation=next;queuedMoves=queuedMoves.slice(1);moveSent=false;
        moveTimeout.restart();Qt.callLater(sendMove);
    }
    function rejectMove() {
        const id=moveOperation.id;
        clearMove();selectedId=id;selectVisibleTask();
        moveMessage='Could not move task: '+errorMessage;
    }
    function startDrag(task, source, width, height, point) {
        if(pendingMove || pendingSave || pendingPurge || completing)return;
        dragSnapshot=stateData;dragSource=source;dragWidth=width;dragHeight=height;
        dragTask=JSON.parse(JSON.stringify(task));moveMessage='';selectedId=task.id;
        updateDrag(point);
    }
    function updateDrag(point) {
        dragPoint=point;dropProject='';
        for(let i=0;i<drawerTargets.count;i++){
            const target=drawerTargets.itemAt(i),p=target.mapFromItem(app,point.x,point.y),d=groupDrawer.mapFromItem(app,point.x,point.y);
            if(groupDrawer.visible && d.y>=0 && d.y<groupDrawer.height && p.x>=0 && p.x<target.width && p.y>=0 && p.y<target.height){
                dropGroup=target.modelData;dropBefore='';return;
            }
        }
        const viewport=taskScroll.mapFromItem(app,point.x,point.y);
        if(viewport.x<0 || viewport.y<0 || viewport.x>taskScroll.width || viewport.y>taskScroll.height){dropGroup='';dropBefore='';return;}
        for(const group of dropGroups) {
            if(!group.visible)continue;
            const local=group.mapFromItem(app,point.x,point.y);
            if(local.x<0 || local.x>group.width || local.y<0 || local.y>group.height)continue;
            let before='';
            for(let i=0;i<group.rows.count;i++) {
                const row=group.rows.itemAt(i);
                if(!row.visible || row.modelData.id===dragTask.id)continue;
                const hit=row.mapFromItem(app,point.x,point.y);
                if(dragTask.kind==='todo' && row.modelData.kind==='project' && hit.x>=0 && hit.x<row.width && hit.y>=0 && hit.y<row.height){dropProject=row.modelData.id;dropGroup=row.modelData.group;dropBefore='';return;}
                if(local.y<row.y+row.gapHeight+(row.height-row.gapHeight)/2){before=row.modelData.id;break;}
            }
            dropGroup=group.modelData;dropBefore=before;return;
        }
        dropGroup='';dropBefore='';
    }
    function finishDrag() {
        if(!dragTask)return;
        if(!dropGroup){cancelDrag();return;}
        moveOperation={id:dragTask.id,revision:dragTask.revision,group:dropGroup,before_id:sortOrder===4?dropBefore:''};
        if(dropProject)moveOperation.project=dropProject;
        pendingMove=true;
        cancelDrag();
        moveTimeout.restart();sendMove();
    }
    function sendMove() {
        if(!pendingMove || moveSent || requestBusy)return;
        const current=(stateData.tasks || []).find(t=>t.id===moveOperation.id);
        if(!current){rejectMove();return;}
        moveOperation=Object.assign({},moveOperation,{revision:current.revision});
        moveSent=true;
        if(moveOperation.project!==undefined)request({action:'edit',id:moveOperation.id,revision:moveOperation.revision,fields:{project:moveOperation.project,group:moveOperation.group}});
        else request(Object.assign({action:'move'},moveOperation));
    }
    Timer {interval:50;repeat:true;running:app.pendingMove && !app.moveSent;onTriggered:app.sendMove()}
    Timer {id:moveTimeout;interval:15000;onTriggered:{app.clearMove();app.moveMessage='Move was not confirmed; refreshing tasks.';app.request({action:'list'});}}
    Timer {
        interval:30;repeat:true;running:!!app.dragTask && !app.pendingMove
        onTriggered:{
            const drawerPoint=groupDrawer.mapFromItem(app,app.dragPoint.x,app.dragPoint.y);
            if(groupDrawer.opened && drawerPoint.y>=0 && drawerPoint.y<groupDrawer.height)return;
            const p=taskScroll.mapFromItem(app,app.dragPoint.x,app.dragPoint.y);
            const flick=taskScroll.contentItem;
            if(p.x<0 || p.x>taskScroll.width || !flick || flick.contentY===undefined)return;
            const delta=p.y<36?-10:p.y>taskScroll.height-36?10:0;
            if(delta){flick.contentY=Math.max(0,Math.min(flick.contentY+delta,flick.contentHeight-flick.height));app.updateDrag(app.dragPoint);}
        }
    }
    Rectangle {
        z:200;visible:!!app.dragTask && !app.pendingMove
        x:app.dragPoint.x+14;y:app.dragPoint.y+14
        width:Math.min(app.dragWidth,360);height:dragPreviewTitle.implicitHeight+20;radius:6;color:theme.surface
        Label {
            id:dragPreviewTitle
            anchors.left:parent.left;anchors.right:parent.right;anchors.top:parent.top;anchors.margins:10
            text:app.dragTask?app.dragTask.title:'';color:theme.text;font.pixelSize:15
            font.strikeout:!!app.dragTask && app.dragTask.done
            wrapMode:Text.Wrap;maximumLineCount:3;elide:Text.ElideRight
        }
    }
    Rectangle {
        id:groupDrawer;objectName:'drag-group-drawer';z:190
        readonly property var groups:app.orderedGroups().filter(g=>app.tasks(g).length===0)
        readonly property bool opened:!!app.dragTask && groups.length>0
        x:8;width:app.width-16;height:Math.min(220,drawerFlow.implicitHeight+48)
        y:opened?app.height-height-8:app.height+8
        visible:opened || y<app.height
        Behavior on y {NumberAnimation {duration:160;easing.type:Easing.OutCubic}}
        radius:8;color:theme.background;border.color:theme.border
        Label {x:14;y:10;text:'Move to another group';color:theme.muted;font.pixelSize:12}
        KineticScrollView {
            anchors.fill:parent;anchors.margins:12;anchors.topMargin:34;clip:true;contentWidth:availableWidth
            Flow {
                id:drawerFlow;width:parent.width;spacing:8
                Repeater {
                    id:drawerTargets;model:groupDrawer.groups
                    Rectangle {
                        required property string modelData
                        objectName:'drag-group-'+modelData
                        width:Math.min(drawerFlow.width,Math.max(100,groupLabel.implicitWidth+28));height:36;radius:5
                        color:app.dropGroup===modelData?Qt.alpha(theme.accent,0.18):theme.surface
                        border.color:app.dropGroup===modelData?theme.accent:'transparent'
                        Label {id:groupLabel;anchors.centerIn:parent;text:parent.modelData==='All'?'Inbox':parent.modelData;color:theme.text;font.pixelSize:13}
                    }
                }
            }
        }
    }
    onShowCompletedChanged: selectVisibleTask()
    onShowCancelledChanged: selectVisibleTask()
    property string filter: ''
    property string selectedId: ''
    property bool taskSelectionActive:true
    property string pendingD:''
    Timer {id:doubleDTimer;interval:app.doubleDInterval;onTriggered:{const id=app.pendingD;app.pendingD='';if(id && app.taskSelectionActive && app.selectedId===id && !app.editor && !taskSettings.visible && app.selected && !['project','template'].includes(app.selected.kind))app.toggleCompletion(id,'');}}
    function pressD() {
        if(!taskSelectionActive || !selected)return;
        if(selectedSubtaskId){toggleCompletion(selectedId,selectedSubtaskId);return;}
        if(doubleDTimer.running && pendingD===selectedId){doubleDTimer.stop();pendingD='';purgeSelected();}
        else {pendingD=selectedId;doubleDTimer.restart();}
    }
    function defocusTask() {
        if(editor || groupRename || dragTask)return;
        taskSelectionActive=false;
        doubleDTimer.stop();pendingD='';
        filterInput.focus=false;
        listKeyboardFocus.forceActiveFocus();
    }
    // Only unhandled clicks reach this background; controls keep their actions.
    MouseArea {
        anchors.fill:parent;z:-1
        enabled:!app.editor && !app.groupRename && !app.dragTask
        onClicked:app.defocusTask()
    }
    function reorderTask(direction) {
        if(!selected || pendingSave || pendingPurge || completing || dragTask || (pendingMove && !moveOperation.keyboard))return;
        sortOrder=4;
        const manual=tasks(selected.group).filter(t=>(t.project || '')===(selected.project || '') && t.kind===selected.kind);
        const at=manual.findIndex(t=>t.id===selectedId),to=at+direction;
        if(to<0 || to>=manual.length)return;
        const before=direction<0?manual[to]:manual[to+1];
        const operation={id:selected.id,revision:selected.revision,group:selected.group,before_id:before?before.id:'',keyboard:true};
        if(pendingMove){queuedMoves=queuedMoves.concat([operation]);return;}
        moveOperation=operation;pendingMove=true;moveTimeout.restart();sendMove();
    }
    property var editing: null
    property string editor: ''
    property string snoozeTaskId: ''
    onSelectedSubtaskIdChanged: {doubleDTimer.stop();pendingD='';}
    onSelectedIdChanged: {snoozeTaskId='';doubleDTimer.stop();pendingD='';}
    function orderedGroups() {
        return (listState?.groups || []).slice().sort((a,b)=>Number(b==='All' || b==='Inbox')-Number(a==='All' || a==='Inbox'));
    }
    function visualRows() {
        const rows=[];
        // Navigation compares these positions with Flickable.contentY. Map to
        // its scrolling content, so row coordinates stay stable while scrolling.
        const content=taskScroll.contentItem.contentItem;
        for(const group of dropGroups)for(let i=0;i<group.rows.count;i++) {
            const row=group.rows.itemAt(i);if(!row.visible)continue;const p=row.mapToItem(content,0,0);
            rows.push({id:row.modelData.id,x:p.x,y:p.y,height:row.height});
        }
        return rows.sort((a,b)=>Math.abs(a.x-b.x)>1?a.x-b.x:a.y-b.y);
    }
    function navigationRows() {
        const rows=[];
        for(const task of visualRows()) {
            rows.push(Object.assign({},task,{subtaskId:'',height:Math.min(task.height,48)}));
            for(const group of dropGroups)for(let i=0;i<group.rows.count;i++) {
                const card=group.rows.itemAt(i);if(card.modelData.id!==task.id)continue;
                for(const item of card.navigationItems()) {const p=item.mapToItem(taskScroll.contentItem.contentItem,0,0);rows.push({id:task.id,subtaskId:item.modelData.id,x:p.x,y:p.y,height:item.height,item:item});}
            }
        }
        return rows;
    }
    function focusRow(next) {
        if(!next)return;
        selectedId=next.id;focusList();selectedSubtaskId=next.subtaskId || '';
        taskScroll.stopWheelScroll();
        const flick=taskScroll.contentItem;
        if(next.y<flick.contentY)flick.contentY=next.y;
        else if(next.y+next.height>flick.contentY+flick.height)flick.contentY=Math.max(0,Math.min(flick.contentHeight-flick.height,next.y+next.height-flick.height));
    }
    function focusVertical(direction,modifiers) {
        if(!taskSelectionActive){restoreTaskFocus();return;}
        if(modifiers & Qt.ShiftModifier){focusList();reorderTask(direction);return;}
        if(collection==='reading'){readingPane.navigate(direction,false);return;}
        const rows=(modifiers & Qt.ControlModifier)?visualRows().map(r=>Object.assign({},r,{subtaskId:''})):navigationRows();
        if(!rows.length)return;
        const index=rows.findIndex(r=>r.id===selectedId && ((modifiers & Qt.ControlModifier) || r.subtaskId===selectedSubtaskId));
        focusRow(rows[index<0?(direction>0?0:rows.length-1):(index+direction+rows.length)%rows.length]);
    }
    function focusColumn(direction) {
        if(!taskSelectionActive){restoreTaskFocus();return;}
        if(collection==='reading'){readingPane.navigate(direction,true);return;}
        const rows=visualRows();
        const current=rows.find(r=>r.id===selectedId);
        if(!current)return;
        const candidates=rows.filter(r=>(r.x-current.x)*direction>1);
        if(!candidates.length)return;
        const distance=Math.min(...candidates.map(r=>Math.abs(r.x-current.x)));
        const adjacent=candidates.filter(r=>Math.abs(Math.abs(r.x-current.x)-distance)<1);
        adjacent.sort((a,b)=>Math.abs(a.y-current.y)-Math.abs(b.y-current.y));
        focusRow(adjacent[0]);
    }
    property var selected: (stateData.tasks || []).find(t=>t.id===selectedId) || null
    readonly property bool imageDetails: taskSelectionActive && !editor && collection!=='reading' && !dragTask && !!selected && (selected.images || []).length>0
    readonly property int selectedColumn: selected ? (groupColumns.placements[selected.group] ?? Math.max(0,groupColumns.groups.indexOf(selected.group))) % groupColumns.columnCount : 0
    readonly property bool imagesOnLeft: selectedColumn>=Math.ceil(groupColumns.columnCount/2)
    onImageDetailsChanged: {if(!imageDetails)balanceTimer.restart();}
    function pasteImage() {
        if(!selected || requestBusy || pendingSave || pendingPurge || pendingMove || clipboardImage.running)return;
        clipboardImage.taskId=selected.id;clipboardImage.revision=selected.revision;
        clipboardImage.output='';clipboardImage.failure='';errorMessage='';clipboardImage.running=true;
    }
    Shortcut {sequence:'Ctrl+V';autoRepeat:false;enabled:app.visible && !app.dragTask && !app.editor && !app.searching && !help.visible && !dateHelp.visible && app.collection!=='reading' && !!app.selected;onActivated:app.pasteImage()}
    Process {
        id:clipboardImage
        property string taskId
        property int revision
        property string output
        property string failure
        command:[Quickshell.env('TASKTJIK_HOME') ? Quickshell.env('TASKTJIK_HOME')+'/tasktjik-api' : 'tasktjik','clipboard-image']
        stdout:StdioCollector {onStreamFinished:clipboardImage.output=text}
        stderr:StdioCollector {onStreamFinished:clipboardImage.failure=text.trim()}
        onExited:(code,status)=>{
            if(code===0) {
                try {app.request({action:'attach-image',id:taskId,revision:revision,data:JSON.parse(output).data});}
                catch(e){app.errorMessage='Could not read clipboard image';}
            } else app.errorMessage=failure || 'Could not read clipboard image';
        }
    }
    property string errorMessage: ""
    property var plans: []
    property var sections: ({})
    function reveal(section) {
        sections = Object.assign({}, sections, {[section]: true});
        Qt.callLater(() => {
            if(section==='repeat')repeatTyping=true;
            const inputs = {group:groupInput,tags:tagsInput,due:dueInput,repeat:repeatInput,reference:planInput,reminder:notifyInput};
            if(inputs[section])inputs[section].forceActiveFocus();
        });
    }
    component QuietButton: Button {
        id: control
        property bool primary: false
        property bool quiet: false
        Keys.onReturnPressed: event=>{if(!event.isAutoRepeat)clicked();event.accepted=true;}
        Keys.onEnterPressed: event=>{if(!event.isAutoRepeat)clicked();event.accepted=true;}
        implicitHeight: 38
        padding: 10
        horizontalPadding: 14
        contentItem: Text {text:control.text;color:!control.enabled?theme.muted:control.primary?theme.background:theme.text;font.pixelSize:13;horizontalAlignment:Text.AlignHCenter;verticalAlignment:Text.AlignVCenter}
        background: Rectangle {radius:10;color:control.primary?theme.accent:control.down?theme.border:control.hovered?theme.surface:'transparent';opacity:control.enabled?1:0.45;border.color:control.activeFocus?theme.accent:control.primary || control.quiet?'transparent':theme.border}
    }
    component IconButton: Button {
        id: control
        property string symbol
        property string description
        implicitWidth:38;implicitHeight:38
        Accessible.name:description
        TaskToolTip {theme:app.theme;visible:control.hovered;text:control.description}
        background:Rectangle {
            radius:8
            color:control.checked?theme.surface:control.hovered?theme.border:'transparent'
            border.color:'transparent'
        }
        contentItem:Canvas {
            id:iconCanvas
            implicitWidth:22;implicitHeight:22
            onWidthChanged:requestPaint()
            onHeightChanged:requestPaint()
            Connections {target:control;function onCheckedChanged(){iconCanvas.requestPaint();}function onSymbolChanged(){iconCanvas.requestPaint();}}
            onPaint:{
                const c=getContext('2d');c.reset();
                c.translate((width-22)/2,(height-22)/2);
                c.strokeStyle=control.checked?theme.accent:theme.text;c.fillStyle=c.strokeStyle;
                c.lineWidth=1.5;c.lineCap='round';c.lineJoin='round';
                if(control.symbol==='search') {
                    c.beginPath();c.arc(9,9,6,0,Math.PI*2);c.moveTo(14,14);c.lineTo(20,20);c.stroke();
                } else if(control.symbol==='edit') {
                    c.beginPath();c.moveTo(4,14);c.lineTo(14,4);c.lineTo(18,8);c.lineTo(8,18);c.lineTo(3,19);c.closePath();c.moveTo(12,6);c.lineTo(16,10);c.moveTo(4,14);c.lineTo(8,18);c.stroke();
                } else if(control.symbol==='blocked') {
                    c.strokeRect(5,9,12,10);c.beginPath();c.arc(11,9,4,Math.PI,2*Math.PI);c.moveTo(11,13);c.lineTo(11,16);c.stroke();
                } else if(control.symbol==='settings') {
                    c.beginPath();c.arc(11,11,6,0,Math.PI*2);c.moveTo(14,11);c.arc(11,11,3,0,Math.PI*2);c.stroke();
                    for(let i=0;i<8;i++){const a=i*Math.PI/4;c.beginPath();c.moveTo(11+6*Math.cos(a),11+6*Math.sin(a));c.lineTo(11+9*Math.cos(a),11+9*Math.sin(a));c.stroke();}
                } else if(control.symbol==='undo') {
                    c.beginPath();c.moveTo(3,9);c.lineTo(8,4);c.moveTo(3,9);c.lineTo(8,14);c.moveTo(3,9);c.lineTo(13,9);c.bezierCurveTo(22,9,22,19,13,19);c.stroke();
                } else if(control.symbol==='info') {
                    c.beginPath();c.arc(11,11,8,0,Math.PI*2);c.stroke();
                    c.fillRect(10,6,2,2);c.beginPath();c.moveTo(11,10);c.lineTo(11,16);c.stroke();
                } else if(control.symbol==='categories') {
                    for(let y=3;y<=17;y+=7){c.strokeRect(2,y,4,4);c.beginPath();c.moveTo(10,y+2);c.lineTo(20,y+2);c.stroke();}
                } else if(control.symbol==='keyboard') {
                    c.strokeRect(1,4,20,14);
                    for(let y=7;y<=10;y+=3)for(let x=4;x<=18;x+=3)c.fillRect(x,y,1,1);
                    c.beginPath();c.moveTo(6,14);c.lineTo(16,14);c.stroke();
                } else {
                    c.beginPath();c.arc(11,11,8,0,Math.PI*2);c.stroke();
                    c.beginPath();
                    if(control.symbol==='completed'){c.moveTo(6,11);c.lineTo(10,15);c.lineTo(16,7);}
                    else if(control.symbol==='cancelled'){c.moveTo(7,7);c.lineTo(15,15);c.moveTo(15,7);c.lineTo(7,15);}
                    else {c.moveTo(11,10);c.lineTo(11,15);c.fillRect(10.3,6,1.4,1.4);}
                    c.stroke();
                }
            }
        }
    }
    component Entry: TextField {
        color:theme.text;placeholderTextColor:theme.muted
        padding:12;font.pixelSize:14;selectByMouse:true
        background: Rectangle {radius:8;color:theme.background;border.color:parent.activeFocus?theme.accent:theme.border}
    }
    component Choice: ComboBox {
        id:control
        implicitWidth:Math.max(120,implicitContentWidth+48);implicitHeight:40
        padding:10;leftPadding:12;rightPadding:30
        contentItem: TextInput {
            text: control.editable ? control.editText : control.displayText
            readOnly: !control.editable
            enabled: control.editable
            color: theme.text; font.pixelSize: 13; verticalAlignment: Text.AlignVCenter
            selectByMouse: control.editable
            onTextEdited: control.editText = text
        }
        indicator: Text {text:'⌄';color:theme.muted;rightPadding:12;anchors.right:parent.right;anchors.verticalCenter:parent.verticalCenter}
        background: Rectangle {radius:10;color:control.hovered?theme.border:theme.surface;border.color:control.activeFocus?theme.accent:theme.border}
        delegate: ItemDelegate {
            width:control.width;text:control.textAt(index)
            contentItem: Text {text:parent.text;color:theme.text;font.pixelSize:13;elide:Text.ElideRight}
            background:Rectangle {radius:6;color:parent.highlighted?theme.border:'transparent'}
            highlighted:control.highlightedIndex===index
        }
        popup: Popup {
            y:control.height+6;width:control.width;padding:6
            implicitHeight:Math.min(contentItem.implicitHeight+12,260)
            background:Rectangle {radius:10;color:theme.surface;border.color:theme.border}
            contentItem:ListView {clip:true;implicitHeight:contentHeight;model:control.popup.visible?control.delegateModel:null;currentIndex:control.highlightedIndex;ScrollIndicator.vertical:ScrollIndicator {}}
        }
    }
    component SearchChoice: QuietButton {
        id:control
        property var model: []
        property int currentIndex: 0
        property string label
        property bool allowEmpty: false
        readonly property string currentValue: model[currentIndex]?.id || ''
        readonly property string currentText: model[currentIndex]?.title || ''
        signal activated(int index)
        implicitHeight:34;horizontalPadding:12
        width:Math.min(implicitWidth,parent.width)
        text:currentValue?label+': '+currentText:'+ '+label
        Accessible.name:text
        contentItem:Text {text:control.text;color:theme.text;font.pixelSize:13;elide:Text.ElideRight;verticalAlignment:Text.AlignVCenter}
        onClicked:picker.open()
        onVisibleChanged:if(!visible)picker.close()
        function choose(index) {
            currentIndex=index;
            activated(index);
            picker.close();
        }
        Popup {
            id:picker;objectName:control.objectName+'-popup'
            x:Math.min(0,app.width-control.mapToItem(app,0,0).x-width-12)
            y:control.height+6;width:Math.min(420,app.width-32);padding:8;margins:12
            implicitHeight:contentItem.implicitHeight+16
            modal:true;focus:true
            closePolicy:Popup.CloseOnEscape | Popup.CloseOnPressOutside
            onOpened:{query.text='';results.currentIndex=0;query.forceActiveFocus();}
            onClosed:if(control.visible)control.forceActiveFocus()
            background:Rectangle {radius:10;color:theme.surface;border.color:theme.border}
            contentItem:ColumnLayout {
                spacing:6
                Entry {
                    id:query;objectName:control.objectName+'-search'
                    Layout.fillWidth:true;placeholderText:'Search '+control.label.toLowerCase()+'…'
                    Accessible.name:placeholderText
                    onTextChanged:results.currentIndex=0
                    onAccepted:if(results.count)control.choose(results.model[results.currentIndex].index)
                    Keys.onDownPressed:if(results.count){results.currentIndex=(results.currentIndex+1)%results.count;results.positionViewAtIndex(results.currentIndex,ListView.Contain);}
                    Keys.onUpPressed:if(results.count){results.currentIndex=(results.currentIndex+results.count-1)%results.count;results.positionViewAtIndex(results.currentIndex,ListView.Contain);}
                    Keys.onEscapePressed:picker.close()
                }
                ListView {
                    id:results;objectName:control.objectName+'-results'
                    Layout.fillWidth:true;Layout.preferredHeight:Math.min(contentHeight,220)
                    clip:true;boundsBehavior:Flickable.StopAtBounds
                    model:control.model.map((entry,index)=>({title:entry.title,id:entry.id,index:index})).filter(entry=>(entry.id || control.allowEmpty) && entry.title.toLowerCase().includes(query.text.trim().toLowerCase()))
                    currentIndex:0
                    delegate:ItemDelegate {
                        required property var modelData
                        required property int index
                        width:results.width;height:36
                        text:modelData.title;highlighted:results.currentIndex===index
                        onClicked:control.choose(modelData.index)
                        contentItem:Text {text:parent.text;color:theme.text;font.pixelSize:13;elide:Text.ElideRight;verticalAlignment:Text.AlignVCenter}
                        background:Rectangle {radius:6;color:parent.highlighted || parent.hovered?theme.border:'transparent'}
                    }
                    ScrollIndicator.vertical:ScrollIndicator {}
                }
                Label {visible:results.count===0;Layout.fillWidth:true;text:'No matching '+control.label.toLowerCase()+'.';color:theme.muted;padding:8}
            }
        }
    }
    Shortcut {sequence:'?';onActivated:help.open()}
    Shortcut {sequences:[StandardKey.Undo];enabled:!editor && !searching && !help.visible && !dateHelp.visible;onActivated:app.undoLastAction()}
    Shortcut {sequence:'U';enabled:!editor && !searching && !help.visible && !dateHelp.visible;onActivated:app.undoLastAction()}
    Shortcut {sequence:'P';autoRepeat:false;enabled:!editor && !searching && !help.visible && !dateHelp.visible && !!selected;onActivated:app.purgeSelected()}
    Shortcut {sequence:'Alt+G';enabled:editor==='edit';onActivated:reveal('group')}
    Shortcut {sequence:'Alt+T';enabled:editor==='edit';onActivated:reveal('tags')}
    Shortcut {sequence:'Alt+D';enabled:editor==='edit';onActivated:reveal('due')}
    Shortcut {sequence:'Alt+R';enabled:editor==='edit';onActivated:reveal('repeat')}
    Shortcut {sequence:'Alt+L';enabled:editor==='edit';onActivated:reveal('reference')}
    Shortcut {sequence:'Alt+M';enabled:editor==='edit';onActivated:reveal('reminder')}
    Shortcut {sequence:'Ctrl+Return';enabled:editor==='edit' && titleInput.text.trim()!=='';onActivated:saveEditor()}
    Popup {
        id:dateHelp
        width:Math.min(460,app.width-32);height:Math.min(implicitHeight,app.height-32)
        anchors.centerIn:parent;padding:22;modal:true;focus:true
        closePolicy:Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background:Rectangle {color:theme.surface;radius:14}
        contentItem:ColumnLayout {
            spacing:16
            Label {text:'Due dates & snoozing';font.pixelSize:20;color:theme.text}
            Label {
                Layout.fillWidth:true;wrapMode:Text.Wrap;color:theme.text
                text:'MON–SUN  ·  Weekday in Today / Week views, or for dates this week in All tasks.\n\n7D  ·  Days, under 10 days\n2W  ·  Weeks, under 3 months\n4M  ·  Months, under 12 months\n1Y  ·  Years\n\n·  No due date\nRed  ·  Overdue\nUnderlined  ·  Snoozed until that date\n›  ·  Focused task; replaces its date label\n\nPress S to slide out snooze choices below a task. Later opens a calendar. The snoozed date takes the place of the due date in the list.'
            }
            QuietButton {text:'Done';quiet:true;onClicked:dateHelp.close()}
        }
    }
    Popup {
        id:help
        x:0;y:0;width:app.width;height:app.height
        padding:24;modal:true;focus:true
        closePolicy:Popup.CloseOnEscape
        background:Rectangle {color:theme.background;radius:14;border.color:theme.border}
        contentItem:ColumnLayout {
            spacing:20
            RowLayout {
                Layout.fillWidth:true
                Label {text:'Keyboard shortcuts';font.pixelSize:20;color:theme.text;Layout.fillWidth:true}
                QuietButton {text:'Done';onClicked:help.close()}
            }
            KineticScrollView {
                Layout.fillWidth:true;Layout.fillHeight:true
                contentWidth:availableWidth
                clip:true
                GridLayout {
                    width:parent.width
                    columns:2;columnSpacing:24;rowSpacing:12
                    Repeater {
                        model:editor==='edit'?[
                            ['⌥ G','Group'],['⌥ T','Tags'],
                            ['⌥ D','Due date'],['⌥ R','Repeat'],
                            ['⌥ L','Reference / note'],['⌥ M','Reminder'],
                            ['C','Cancel task (outside text fields)'],['⌃ ↵','Save'],['⎋','Close editor']
                        ]:collection==='reading'?[
                            ['N','Save a page'],['D','Mark read / unread'],
                            ['E / T','Edit page'],['G','Edit category'],
                            ['/','Search pages'],['↵','Open page'],
                            ['← →','Previous / next page'],['↑ ↓ / J K','Select row'],
                            ['P','Remove page'],['U / Ctrl+Z','Undo'],['⎋','Close window']
                        ]:[
                            ['N','New item'],['⇧ N','Today'],
                            ['W','This week'],['M','This month'],['L','All tasks'],
                            ['D','Mark done'],['D D','Delete task'],['C','Cancel task'],['P','Purge task'],['U / Ctrl+Z','Undo'],
                            ['E / T','Edit task'],['G','Edit group'],
                            ['S','Snooze'],['/','Search'],
                            ['Ctrl+V','Attach clipboard image'],
                            ['← →','Nearest task in adjacent column'],['↑ ↓ / J K','Select item'],['Ctrl+Left','Back from project'],['Ctrl+↑ ↓','Skip subtasks'],['Shift+↑ ↓','Reorder manually'],['↵','Open project / reading reference'],
                            ['⎋','Close window']
                        ]
                        delegate:RowLayout {
                            required property var modelData
                            Layout.fillWidth:true
                            Layout.preferredWidth:1
                            spacing:12
                            Rectangle {
                                Layout.preferredWidth:80;Layout.preferredHeight:36
                                radius:8;color:theme.surface;border.color:theme.border
                                Text {anchors.centerIn:parent;text:modelData[0];color:theme.text;font.pixelSize:15}
                            }
                            Label {text:modelData[1];color:theme.text;font.pixelSize:13;wrapMode:Text.Wrap;Layout.fillWidth:true}
                        }
                    }
                }
            }
        }
    }

    property var pendingPurge: null
    property var queuedPurges: []
    property var purgeHistory: []
    property string undoToastTask: ''
    property string undoToastTitle: ''
    function purgeSelected() {
        if(selectedSubtaskId || !taskSelectionActive)return;
        if(!selected || pendingSave || pendingMove)return;
        if([pendingPurge].concat(queuedPurges).some(p=>p && p.id===selected.id))return;
        const operation={id:selected.id,title:selected.title,revision:selected.revision,
            task:JSON.parse(JSON.stringify(selected)),sent:false,confirmed:false,
            undoRequested:false,undoSent:false,requestId:newRequestId(),undoId:newRequestId()};
        if(pendingPurge)queuedPurges=queuedPurges.concat([operation]);
        else pendingPurge=operation;
        moveMessage='';
        undoToastTask=operation.id;undoToastTitle=operation.title;toastTimer.restart();
        selectVisibleTask();
        sendPurge();
    }
    function sendPurge() {
        if(!pendingPurge || requestBusy)return;
        if(!pendingPurge.sent) {
            pendingPurge=Object.assign({},pendingPurge,{sent:true});
            request({action:'purge',id:pendingPurge.id,revision:pendingPurge.revision,request_id:pendingPurge.requestId});
        } else if(pendingPurge.confirmed && pendingPurge.undoRequested && !pendingPurge.undoSent) {
            pendingPurge=Object.assign({},pendingPurge,{undoSent:true});
            request({action:'undo',id:pendingPurge.id,undo_action:'purge',request_id:pendingPurge.undoId});
        }
    }
    function advancePurge() {
        pendingPurge=queuedPurges.length?queuedPurges[0]:null;
        queuedPurges=queuedPurges.slice(1);
    }
    function rejectPurge() {
        if(!pendingPurge)return;
        const operation=pendingPurge;
        if(operation.undoSent) {
            // A failed restore leaves the server's undo stack unchanged. Later
            // restores depend on it, so roll them back and retain their order.
            const restores=queuedPurges.filter(p=>p.undoRequested).reverse();
            purgeHistory=purgeHistory.concat(restores,[operation]).slice(-100);
            queuedPurges=queuedPurges.filter(p=>!p.undoRequested);
        }
        if(undoToastTask===operation.id){undoToastTask='';toastTimer.stop();}
        moveMessage=(operation.undoSent?'Could not restore “':'Could not delete “')+operation.title+'”: '+errorMessage;
        advancePurge();
        if(!operation.undoSent)selectedId=operation.id;
        selectVisibleTask();
    }
    Timer {interval:25;repeat:true;running:!!app.pendingPurge && (!app.pendingPurge.sent || (app.pendingPurge.confirmed && app.pendingPurge.undoRequested && !app.pendingPurge.undoSent));onTriggered:app.sendPurge()}
    function undoLastAction() {
        if(pendingSave || pendingMove || completing)return;
        // Newest queued deletion can be cancelled without ever reaching the server.
        const waiting=queuedPurges.map(p=>p.id).lastIndexOf(undoToastTask);
        let index=waiting;
        if(index<0)for(let i=queuedPurges.length-1;i>=0;i--)if(!queuedPurges[i].undoRequested){index=i;break;}
        let operation=index>=0?queuedPurges[index]:pendingPurge && !pendingPurge.undoRequested?pendingPurge:null;
        if(operation) {
            if(index>=0)queuedPurges=queuedPurges.filter((p,i)=>i!==index);
            else if(!operation.sent)advancePurge();
            else pendingPurge=Object.assign({},operation,{undoRequested:true});
        } else if(purgeHistory.length) {
            operation=purgeHistory[purgeHistory.length-1];
            purgeHistory=purgeHistory.slice(0,-1);
            const restore=Object.assign({},operation,{sent:true,confirmed:true,undoRequested:true,undoSent:false});
            if(pendingPurge)queuedPurges=queuedPurges.concat([restore]);
            else pendingPurge=restore;
        }
        const task=undoToastTask;
        undoToastTask='';toastTimer.stop();
        if(operation) {
            selectedId=operation.id;moveMessage='';app.focusList();
            sendPurge();
        } else if(!pendingPurge)request(task?{action:'undo',id:task,undo_action:'purge'}:{action:'undo'});
    }
    onErrorMessageChanged: {if(errorMessage){if(completionQueue.length && completionQueue[0].sent)rejectCompletion();cancellingId='';if(pendingPurge && (pendingPurge.undoSent || (pendingPurge.sent && !pendingPurge.confirmed)))rejectPurge();if(pendingMove && moveSent)rejectMove();if(dragTask)cancelDrag();if(pendingSave && pendingSave.sent)rejectSave();}}
    Timer {id:toastTimer;interval:3000;onTriggered:app.undoToastTask='';}
    Rectangle {
        id:purgeToast
        z:100
        anchors.horizontalCenter:parent.horizontalCenter
        anchors.bottom:parent.bottom;anchors.bottomMargin:8
        width:Math.min(app.width-16,440);height:56;radius:12
        color:theme.surface
        border.color:theme.border
        visible:app.undoToastTask!==''
        RowLayout {
            anchors.fill:parent;anchors.margins:8;anchors.leftMargin:16;spacing:6
            Label {
                text:'Deleted';color:theme.muted
                font.pixelSize:14;renderType:Text.NativeRendering
            }
            Label {
                Layout.fillWidth:true;Layout.minimumWidth:0
                text:'“'+app.undoToastTitle+'”';textFormat:Text.PlainText
                elide:Text.ElideRight;color:theme.text
                font.pixelSize:14;font.weight:Font.DemiBold;renderType:Text.NativeRendering
            }
            Button {
                id:toastUndo
                text:'Undo';Layout.leftMargin:8
                implicitWidth:68;implicitHeight:38
                Accessible.name:'Undo deletion'
                contentItem:Text {
                    text:toastUndo.text;color:theme.accent
                    font.pixelSize:14;font.weight:Font.DemiBold;renderType:Text.NativeRendering
                    horizontalAlignment:Text.AlignHCenter;verticalAlignment:Text.AlignVCenter
                }
                background:Rectangle {
                    radius:8
                    color:Qt.alpha(theme.accent,toastUndo.down?0.22:toastUndo.hovered?0.16:0.08)
                    border.color:toastUndo.visualFocus?theme.accent:'transparent'
                }
                onClicked:app.undoLastAction()
            }
        }
    }
    signal request(var request)
    onRequest: value=>{if(!['list','refresh','plans','open-reference','purge','undo'].includes(value.action))purgeHistory=[];}
    signal exitApp()
    signal openWindow()
    property bool canOpenWindow: false
    readonly property bool settingsOpen: taskSettings.visible
    property var groupRename:null
    function beginGroupRename(group) {
        if(requestBusy || groupRename && groupRename.sent)return;
        groupRename={source:group,sent:false,error:'',request:{action:'group-rename',source:group,groupsRevision:stateData.groupsRevision || 0,scoped:true,
            members:tasks(group).map(t=>({id:t.id,revision:t.revision}))}};
    }
    function saveGroupRename(target) {
        if(!groupRename || groupRename.sent || requestBusy || !target.trim())return;
        target=target.trim();
        if(target===groupRename.source){groupRename=null;focusList();return;}
        groupRename=Object.assign({},groupRename,{target:target,sent:true,error:''});
        request(Object.assign({},groupRename.request,{target:target}));
    }
    Connections {target:app
        function onErrorMessageChanged(){if(app.errorMessage && app.groupRename && app.groupRename.sent)app.groupRename=Object.assign({},app.groupRename,{sent:false,error:app.errorMessage});}
        function onStateDataChanged(){if(app.groupRename && app.groupRename.sent && app.groupRename.request.members.every(member=>(app.stateData.tasks || []).some(t=>t.id===member.id && t.group===app.groupRename.target && t.revision>member.revision))){app.groupRename=null;app.focusList();}}
    }
    property string cancellingId: ''
    function cancelTask() {
        if(!editing || !editing.id || editing.kind!=='todo' || requestBusy)return;
        cancellingId=editing.id;
        request({action:'status',id:editing.id,revision:editing.revision,period:editing.period,status:editing.status==='cancelled'?'ready':'cancelled',actor:'user'});
    }
    Shortcut {
        sequence:'C';autoRepeat:false
        enabled:app.editor==='edit' && !!app.editing && app.editing.kind==='todo' && !!app.editing.id && !app.requestBusy && !(app.Window.window && app.Window.window.activeFocusItem && app.Window.window.activeFocusItem.cursorPosition!==undefined)
        onActivated:app.cancelTask()
    }
    TaskSettings {
        id:taskSettings;objectName:'task-settings';parentWindow:app.QsWindow.window
        theme:app.theme;stateData:app.stateData;busy:app.requestBusy;errorMessage:app.errorMessage
        doubleDInterval:app.doubleDInterval
        onDoubleDIntervalSelected:value=>app.doubleDInterval=value
        domainSort:app.domainSort
        domainGroups:app.manualGroups()
        onDomainSortSelected:value=>app.domainSort=value
        onMoveDomain:(group,direction)=>app.moveDomain(group,direction)
        projectSort:app.projectSort
        onProjectSortSelected:value=>app.projectSort=value
        onOperation:value=>app.request(value)
        onEditTemplate:task=>{app.selectedId=task.id;app.openEditor(task);}
        onNewTemplate:app.newItem('template')
    }
    function action(name, extra) {if(name==='toggle' && selected){toggleCompletion(selected.id,'');return;}if(completing)return;if(selected && !pendingSave && !pendingPurge && !pendingMove)request(Object.assign({action:name,id:selected.id,period:selected.period,revision:selected.revision},extra || {}));}
    function openEditor(draft) {
        if(pendingSave || pendingPurge || pendingMove || completing)return;
        selectedSubtaskId='';dueToday=new Date();
        retrySave=null;
        editing=draft || (selected ? JSON.parse(JSON.stringify(selected)) : {title:'',kind:collection,project:projectId,group:currentProject?currentProject.group:'All',tags:[],due:'',repeat:{},reference:'',subtasks:[]});
        workflowEditor.reset(editing.subtasks || [],editing.workflow===true);templatePicker.currentIndex=0;
        projectPicker.currentIndex=Math.max(0,projectPicker.model.findIndex(p=>p.id===editing.project));
        projectOnly.checked=editing.onlyInProject!==false;
        titleInput.text=editing.title;groupInput.editText=editing.group || 'All';
        tagsInput.text=editing.tags.join(', ');dueInput.text=editing.due;
        repeatInput.text=editing.repeat.unit?Repeat.format(editing.repeat):'';repeatTyping=!editing.repeat.unit;
        planInput.text=editing.reference || editing.plan || '';
        notifyInput.checked=editing.notifyBeforeDue!==false;
        leadValue.text=String(editing.notifyBeforeDueLeadValue ?? 1);
        leadUnit.currentIndex=Math.max(0,leadUnit.model.indexOf(editing.notifyBeforeDueLeadUnit || 'days'));
        sections = {group:!!editing.id && editing.group!=='All',tags:editing.tags.length>0,
            due:!!editing.due,repeat:!!editing.repeat.unit,reference:editing.kind==='reading' || !!planInput.text,
            reminder:!!editing.id && (editing.notifyBeforeDue===false || Number(leadValue.text)!==1 || leadUnit.currentText!=='days')};
        planPicker.visible=false;
        editor='edit';titleInput.forceActiveFocus();
    }
    function cancelEditor() {
        if(pendingSave)return;
        retrySave=null;
        if(draftId)request({action:'reading-cancel',draft_id:draftId});
        else {editor='';app.focusList();}
    }
    function saveEditor() {
        if(editor!=='edit' || pendingSave || !titleInput.text.trim())return;
        if(!workflowEditor.flush())return;
        if(parsedRepeat.error){errorMessage=parsedRepeat.error;repeatTyping=true;repeatInput.forceActiveFocus();return;}
        if(parsedDue.error){errorMessage=parsedDue.error;dueInput.forceActiveFocus();return;}
        const value={action:draftId?'reading-save':editing.id?'edit':'create',draft_id:draftId,
            id:editing.id || '',revision:editing.revision,
            fields:{kind:editing.kind || collection,project:projectPicker.currentValue || '',onlyInProject:projectOnly.checked,subtasks:workflowEditor.nodes,workflow:workflowEditor.sequential,title:titleInput.text,group:groupInput.editText.trim() || 'All',
                tags:tagsInput.text.split(',').map(s=>s.trim()).filter(Boolean),due:parsedDue.value,
                repeat:Object.assign({},parsedRepeat.rule,parsedRepeat.rule.unit && editing.repeat.start?{start:editing.repeat.start}:{}),
                reference:planInput.text,notifyBeforeDue:notifyInput.checked,
                notifyBeforeDueLeadValue:Number(leadValue.text),notifyBeforeDueLeadUnit:leadUnit.currentText}};
        const fingerprint=JSON.stringify(value);
        if(!retrySave || retrySave.fingerprint!==fingerprint) {
            // Keep the same id for an unchanged retry if a reply was lost.
            const id=newRequestId();
            retrySave={fingerprint:fingerprint,id:id};
        }
        value.request_id=draftId || retrySave.id;
        pendingSave={request:value,draft:Object.assign({},editing,value.fields),sections:sections,sent:false};
        if(!draftId){editor='';selectedId=value.id || value.request_id;app.focusList();}
        sendSave();
    }
    function newRequestId() {
        return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g,c=>{
            const n=Math.floor(Math.random()*16);return (c==='x'?n:(n&3)|8).toString(16);
        });
    }
    function sendSave() {
        if(!pendingSave || pendingSave.sent || requestBusy)return;
        pendingSave=Object.assign({},pendingSave,{sent:true});
        request(pendingSave.request);
    }
    Timer {interval:25;repeat:true;running:!!app.pendingSave && !app.pendingSave.sent;onTriggered:app.sendSave()}
    function rejectSave() {
        if(!pendingSave)return;
        const saved=pendingSave,retry=retrySave;
        pendingSave=null;
        openEditor(saved.draft);
        sections=saved.sections;
        retrySave=retry;
    }
    function selectVisibleTask() {
        const rows=collection==='reading'?readingSites():(listState?.groups || []).reduce((items,g)=>items.concat(tasks(g)),[]);
        if(!rows.some(t=>t.id===selectedId))selectedId=rows.length?rows[0].id:'';
    }
    onProjectIdChanged:groupColumns.reset()
    onViewChanged: {groupColumns.reset();if(!projectId && view!=='projects')previousOverviewView=view;selectVisibleTask();}
    onFilterChanged: selectVisibleTask()
    onCollectionChanged: {groupColumns.reset();selectVisibleTask();}
    function readingSites() {
        return (listState?.tasks || []).filter(t=>t.kind==='reading' && (showCompleted || !t.done) && t.status!=='cancelled' && matchesFilter(t)).sort((a,b)=>readingSort===1?a.title.localeCompare(b.title):readingSort===2?String(a.reference || '').replace(/^https?:\/\/(www\.)?/,'').localeCompare(String(b.reference || '').replace(/^https?:\/\/(www\.)?/,'')):String(b.createdAt || '').localeCompare(String(a.createdAt || '')) || b.id.localeCompare(a.id));
    }
    function visibleCount() {if(collection==='reading')return readingSites().length;return (listState?.groups || []).reduce((n,g)=>n+tasks(g).length,0);}
    function taskMeta(task) {
        const parts=[];
        if(task.saving)parts.push('Saving…');
        if(task.kind==='project')return '';
        if(task.project && !projectId){const p=(listState?.tasks || []).find(p=>p.id===task.project);if(p)parts.push(p.title);}
        if(task.status && !['ready','inbox','done','cancelled','blocked'].includes(task.status))parts.push(task.status.replace('_',' '));
        if(task.assignee)parts.push(task.assignee);
        if(task.verification && task.verification.length)parts.push('Check: '+task.verification[task.verification.length-1].outcome);
        return parts.join(' · ');
    }
    function effectiveDue(task) {
        if(task.kind==='project') {
            return (listState?.tasks || []).filter(t=>t.project===task.id && t.kind==='todo' && !t.done && t.status!=='cancelled')
                .map(t=>effectiveDue(t)).filter(Boolean).sort()[0] || '';
        }
        const reminder=(task.workflowRows || []).filter(r=>r.kind==='reminder' && r.status==='ready').map(r=>r.due).sort()[0];
        if(reminder && (!task.due || reminder<task.due))return reminder;
        if(task.snoozedUntil) return Qt.formatDate(new Date(task.snoozedUntil),'yyyy-MM-dd');
        return task.period && task.period!=='once' ? task.period : task.due || '';
    }
    function isOverdue(task) {
        const due=effectiveDue(task);
        return !!due && !task.done && task.status!=='cancelled' && due < (listState?.today || Qt.formatDate(new Date(),'yyyy-MM-dd'));
    }
    function dueLabel(task) {return DateCode.dateCode(effectiveDue(task),listState?.today || Qt.formatDate(new Date(),'yyyy-MM-dd'),view);}
    function compareProjectPlacement(a,b) {
        const difference=Number(a.kind==='project')-Number(b.kind==='project');
        return projectSort==='first'?-difference:projectSort==='last'?difference:0;
    }
    function completionProgress(task) {
        const progress=task.kind==='project'?projectProgress(task):task.subtaskProgress;
        return progress && progress.total>0?progress.done/progress.total:0;
    }
    function compareTasks(a,b) {
        const placement=compareProjectPlacement(a,b);
        if(placement)return placement;
        if(sortOrder===4)return String(a.manualOrder || a.id.replace(/-/g,'')+'8').localeCompare(String(b.manualOrder || b.id.replace(/-/g,'')+'8')) || a.id.localeCompare(b.id);
        if(sortOrder===5)return completionProgress(b)-completionProgress(a) || a.title.localeCompare(b.title) || a.id.localeCompare(b.id);
        if(sortOrder>=2)return (sortOrder===3?-1:1)*a.title.localeCompare(b.title);
        const ad=effectiveDue(a),bd=effectiveDue(b);
        if(!ad || !bd)return ad?-1:bd?1:a.title.localeCompare(b.title);
        return (sortOrder===1?-1:1)*ad.localeCompare(bd) || a.title.localeCompare(b.title);
    }
    Item {id:listKeyboardFocus}
    function restoreTaskFocus() {
        const rows=navigationRows();
        const previous=rows.find(r=>r.id===selectedId && r.subtaskId===selectedSubtaskId)
            || rows.find(r=>r.id===selectedId);
        if(previous)focusRow(previous);else focusList();
    }
    function focusList() {
        taskSelectionActive=true;
        selectedSubtaskId='';
        // Give list shortcuts their own focus target; previously focused buttons
        // must not consume Return after selecting a task or cancelling a drag.
        filterInput.focus=false;
        listKeyboardFocus.forceActiveFocus();
    }
    function filterTag(tag) {
        searching=true;
        filterInput.text='tag:'+( /\s|"/.test(tag) ? JSON.stringify(tag) : tag );
        filterInput.forceActiveFocus();
    }
    function matchesFilter(task) {
        const tags=(task.tags || []).map(t=>t.toLowerCase());
        let matches=true;
        const text=filter.replace(/tag:("(?:\\.|[^"\\])*"|[^\s]+)/gi,(_,token)=>{
            let tag=token;
            if(token.startsWith('"')) {try {tag=JSON.parse(token);}catch(e){matches=false;}}
            if(!tags.includes(tag.toLowerCase()))matches=false;
            return '';
        }).trim().toLowerCase();
        return matches && (!text || (task.title+' '+(task.tags || []).join(' ')+' '+task.group+' '+(task.reference || '')).toLowerCase().includes(text));
    }
    readonly property var tasksByGroup: {
        const result={};
        for(const group of (listState?.groups || []))result[group]=computeTasks(group);
        return result;
    }
    onTasksByGroupChanged: selectVisibleTask()
    function tasks(group) {return tasksByGroup?.[group] || [];}
    function computeTasks(group) {
        const now=listState?.today || Qt.formatDate(new Date(),'yyyy-MM-dd');
        let end=new Date(now+'T12:00:00');if(view==='month')end=new Date(end.getFullYear(),end.getMonth()+1,0,12);else end.setDate(end.getDate()+6-((end.getDay()+6)%7));
        const last=Qt.formatDate(end,'yyyy-MM-dd');
        let rows=(listState?.tasks || []).filter(t=>(collection==='todo' && view==='projects' && !projectId?t.kind==='project':((t.kind || 'todo')===collection || collection==='todo' && t.kind==='project' && !projectId)) && (projectId?t.project===projectId:!t.project || (listState?.tasks || []).find(p=>p.id===t.project)?.onlyInProject!==true) && t.group===group && (showCompleted || !t.done) && (showCancelled || t.status!=='cancelled') && (showBlocked || t.status!=='blocked') && matchesFilter(t) &&
            (t.kind==='project' || collection==='template' || view==='all' || !effectiveDue(t) || (effectiveDue(t) && effectiveDue(t) <= (view==='today'?now:last)) || (t.completedAt && Qt.formatDate(new Date(t.completedAt),'yyyy-MM-dd')===now))).sort(compareTasks);
        if(pendingMove && moveOperation && sortOrder===4)for(const operation of [moveOperation].concat(queuedMoves)) {
            if(group!==operation.group)continue;
            const moved=rows.find(t=>t.id===operation.id);
            if(!moved)continue;
            rows=rows.filter(t=>t.id!==moved.id);
            const before=rows.findIndex(t=>t.id===operation.before_id);
            rows.splice(before<0?rows.length:before,0,moved);
            rows.sort(compareProjectPlacement);
        }
        return rows;
    }
    TaskImages {
        id:imagePane
        z:10
        visible:app.imageDetails
        theme:app.theme
        task:visible?app.selected:null
        x:taskScroll.parent.x+taskScroll.x+(app.imagesOnLeft?0:taskScroll.width/2+16)
        y:taskScroll.parent.y+taskScroll.y
        width:taskScroll.width/2-16
        height:taskScroll.height
        onBackgroundClicked:app.defocusTask()
        // Keep clicks and wheel events inside the details pane.
        MouseArea {anchors.fill:parent;z:-1;onClicked:app.defocusTask();onWheel:wheel=>{wheel.accepted=true;}}
    }
    Keys.onPressed: event=>{
        if(dragTask){if(event.key===Qt.Key_Escape && !pendingMove)cancelDrag();event.accepted=true;return;}
        if(help.visible || dateHelp.visible || taskSettings.visible)return;
        if(groupRename)return;
        // An open filter must not disable list shortcuts after focus returns to a task.
        if(editor || filterInput.activeFocus){if(event.key===Qt.Key_Escape && editor){cancelEditor();event.accepted=true;}return;}
        if(event.key===Qt.Key_N){if(event.modifiers & Qt.ShiftModifier && collection!=='reading')view='today';else newDefault();}
        else if(event.key===Qt.Key_L)view='all';
        else if(event.key===Qt.Key_W && collection!=='reading')view='week';
        else if(event.key===Qt.Key_M && collection!=='reading')view='month';
        else if(event.key===Qt.Key_D && !(event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier))){if(!event.isAutoRepeat)pressD();}
        else if(event.key===Qt.Key_C && !(event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier))){if(!event.isAutoRepeat)cancelFocusedTask();}
        else if((event.key===Qt.Key_Return || event.key===Qt.Key_Enter || event.key===Qt.Key_Space) && taskSelectionActive && selectedSubtaskId){if(!event.isAutoRepeat)toggleCompletion(selectedId,selectedSubtaskId);}
        else if((event.key===Qt.Key_Return || event.key===Qt.Key_Enter) && selected && selected.kind==='project')enterProject(selected);
        else if((event.key===Qt.Key_Return || event.key===Qt.Key_Enter) && selected && (selected.subtasks || []).length){toggleExpanded(selected.id);}
        else if((event.key===Qt.Key_Return || event.key===Qt.Key_Enter) && collection==='reading')action('open-reference');
        else if(event.key===Qt.Key_E || event.key===Qt.Key_T || event.key===Qt.Key_G){if(selected){openEditor();if(event.key===Qt.Key_G)reveal('group');}}
        else if(event.key===Qt.Key_S){if(selected && !pendingSave)snoozeTaskId=snoozeTaskId===selectedId?'':selectedId;}
        else if(event.key===Qt.Key_Slash){searching=true;filterInput.forceActiveFocus();}
        else if(event.key===Qt.Key_Escape){if(snoozeTaskId)snoozeTaskId='';else if(projectId)leaveProject();else exitApp();}
        else if(event.key===Qt.Key_Left && (event.modifiers & Qt.ControlModifier)){if(projectId)leaveProject();else if(view==='projects')view=previousOverviewView;focusList();}
        else if(event.key===Qt.Key_Left || event.key===Qt.Key_Right)focusColumn(event.key===Qt.Key_Left?-1:1);
        else if([Qt.Key_Down,Qt.Key_Up,Qt.Key_J,Qt.Key_K,Qt.Key_Tab].includes(event.key)) {
            focusVertical([Qt.Key_Up,Qt.Key_K].includes(event.key)?-1:1,event.modifiers);
        }
        event.accepted=true;
    }
    onStateDataChanged: {
        if(cancellingId){const changed=(stateData.tasks || []).find(t=>t.id===cancellingId);if(changed && editing && changed.revision>editing.revision){cancellingId='';editor='';app.focusList();}}
        if(pendingMove && moveOperation && moveSent) {
            const moved=(stateData.tasks || []).find(t=>t.id===moveOperation.id);
            if(moved && moved.revision>moveOperation.revision && (moveOperation.project===undefined || moved.project===moveOperation.project) && moved.group===moveOperation.group && (moved.events || []).slice(-1).some(e=>e.action===(moveOperation.project!==undefined?'edit':'move')))acceptMove();
        }
        if(pendingPurge && pendingPurge.sent && Array.isArray(stateData.tasks)) {
            const present=stateData.tasks.some(t=>t.id===pendingPurge.id);
            if(!pendingPurge.confirmed && !present) {
                pendingPurge=Object.assign({},pendingPurge,{confirmed:true});
                if(!pendingPurge.undoRequested) {
                    purgeHistory=purgeHistory.concat([pendingPurge]).slice(-100);
                    advancePurge();
                }
            } else if(pendingPurge.undoSent && present)advancePurge();
        }
        if(stateData.plans)plans=stateData.plans;
        if(stateData.collection && collection!==stateData.collection){collection=stateData.collection;view=collection==='reading'?'all':'week';}
        if(stateData.draft_id && stateData.draft_id!==lastDraftId){
            draftId=stateData.draft_id;lastDraftId=draftId;
            Qt.callLater(()=>openEditor(stateData.draft));
        }
        selectVisibleTask();
    }
    ColumnLayout {
        anchors.fill:parent;anchors.margins:12;spacing:18
        RowLayout {
            Layout.fillWidth:true;spacing:12
            ColumnLayout {
                Layout.fillWidth:true;spacing:3
                Label {text:currentProject?currentProject.title:collection==='reading'?'Reading':collection==='template'?'Templates':view==='projects' && !projectId?'Projects':'Tasks';font.pixelSize:24;font.weight:Font.DemiBold;color:theme.text}
                Label {text:editor==='edit'?(draftId?'Save this page to your reading list.':editing && editing.id?'Edit details':'Add a new '+(editing && editing.kind==='project'?'project':editing && editing.kind==='template'?'template':collection==='reading'?'reading item':'task')):visibleCount()+' '+(collection==='reading'?'saved pages':view==='projects' && !projectId?'projects':collection==='template'?'templates':('tasks · '+(view==='week'?'This week':view==='today'?'Today':view==='month'?'This month':'All dates')));font.pixelSize:12;color:theme.muted}
            }
            Item {Layout.fillWidth:true}
            QuietButton {text:projectReturnView==='projects'?'‹ Projects':'‹ All tasks';visible:!!projectId && !editor;onClicked:leaveProject()}
            Choice {visible:!editor && !projectId;model:['Todo','Reading'];currentIndex:collection==='reading'?1:0;enabled:!editor;onActivated:{collection=currentIndex===1?'reading':'todo';view=collection==='reading'?'all':'week';selectVisibleTask();}}
            QuietButton {text:'Open window';visible:canOpenWindow && !editor;enabled:!pendingSave;quiet:true;onClicked:app.openWindow()}
            QuietButton {objectName:'new-item';text:collection==='reading'?'+ Save page':'+ New';visible:!editor;primary:true;enabled:!draftId && !pendingSave;Accessible.name:'New '+app.newKind;onClicked:app.newDefault()}
        }
        RowLayout {
            visible:!editor;Layout.fillWidth:true;spacing:4
            Repeater {
                model:collection!=='todo'?[]:[{label:'Today',value:'today'},{label:'This week',value:'week'},{label:'This month',value:'month'},{label:'All tasks',value:'all'}].concat(projectId?[]:[{label:'Projects',value:'projects'}])
                QuietButton {
                    required property var modelData
                    objectName:'view-'+modelData.value
                    text:modelData.label;quiet:true;horizontalPadding:app.width<900?9:14
                    checked:app.view===modelData.value
                    background:Rectangle {radius:9;color:parent.checked?theme.surface:'transparent';border.color:parent.checked?theme.border:'transparent'}
                    onClicked:{app.view=modelData.value;app.focusList();}
                }
            }
            Item {visible:collection!=='reading';Layout.fillWidth:true}
            QuietButton {visible:collection==='reading';text:'List';quiet:app.readingGrid;onClicked:app.readingGrid=false}
            QuietButton {visible:collection==='reading';text:'Grid';quiet:!app.readingGrid;onClicked:app.readingGrid=true}
            Item {visible:collection==='reading';Layout.fillWidth:true}
            Choice {visible:collection==='reading';model:['Recently saved','Title: A–Z','Site: A–Z'];currentIndex:app.readingSort;implicitWidth:145;onActivated:app.readingSort=currentIndex}
            Choice {objectName:'sortChoice';visible:collection!=='reading';model:['Due: soonest','Due: latest','Title: A–Z','Title: Z–A','Manual','Progress'];currentIndex:app.sortOrder;implicitWidth:140;onActivated:app.sortOrder=currentIndex}
            IconButton {symbol:'search';description:collection==='reading'?'Filter saved pages (/)':'Filter tasks (/)' ;checkable:true;checked:app.searching;onClicked:{app.searching=checked;if(checked)filterInput.forceActiveFocus();else {filterInput.text='';app.focusList();}}}
            IconButton {symbol:'completed';description:collection==='reading'?'Show read pages':'Show completed tasks';checkable:true;checked:app.showCompleted;onClicked:app.showCompleted=checked}
            IconButton {symbol:collection==='reading'?'categories':'cancelled';description:collection==='reading'?'Group pages by category':'Show cancelled tasks';checkable:true;checked:collection==='reading'?app.readingCategories:app.showCancelled;onClicked:{if(collection==='reading')app.readingCategories=checked;else app.showCancelled=checked}}
            IconButton {objectName:'blocked-toggle';visible:collection!=='reading';symbol:'blocked';description:'Show blocked tasks and workflow steps';checkable:true;checked:app.showBlocked;onClicked:app.showBlocked=checked}
            IconButton {visible:collection!=='reading';symbol:'details';description:'Show selected task details';checkable:true;checked:app.showDetails;onClicked:app.showDetails=checked}
        }
        Entry {color:theme.text;placeholderTextColor:theme.muted;id:filterInput;visible:!editor && searching;Layout.fillWidth:true;placeholderText:collection==='reading'?'Filter reading list':'Filter tasks';onTextChanged:app.filter=text;Keys.onEscapePressed:{searching=false;filter='';text='';app.focusList();}}
        Label {
            visible:!editor && showDetails && !!selected && text!==''
            Layout.fillWidth:true;wrapMode:Text.Wrap;color:theme.muted
            text:selected ? (selected.blockers || []).map(b=>'Blocked: '+b.reason+(b.task_id?' ['+b.task_id+']':'')).concat(
                (selected.acceptance || []).map(c=>'Accept: '+c),
                (selected.verification || []).slice(-1).map(v=>v.outcome+' by '+v.actor+' · '+v.artifact+' · '+v.evidence)).join('\n') : ''
        }
        ReadingView {
            id:readingPane
            ScrollBar.vertical: EdgeScrollBar {scrollView:readingPane}
            visible:!editor && collection==='reading';Layout.fillWidth:true;Layout.fillHeight:true
            theme:app.theme;sites:visible?app.readingSites():[];grid:app.readingGrid;categories:app.readingCategories;selectedId:app.selectedId
            onSelectSite:id=>{app.selectedId=id;app.focusList();}
            onOpenSite:id=>{app.selectedId=id;app.action('open-reference');}
            onBackgroundClicked:app.defocusTask()
        }
        KineticScrollView {
            id: taskScroll;objectName:'task-scroll'
            ScrollBar.vertical: EdgeScrollBar {scrollView:taskScroll}
            visible:!editor && collection!=='reading';Layout.fillWidth:true;Layout.fillHeight:true;clip:true
            contentWidth:availableWidth
            ScrollBar.horizontal.policy:ScrollBar.AlwaysOff
            onBackgroundClicked:app.defocusTask()
            ColumnLayout {
                width:taskScroll.availableWidth;spacing:14
                Item {
                    id: groupColumns
                    Layout.fillWidth:true
                    implicitHeight:app.dropGroups.reduce((bottom,group)=>group.visible?Math.max(bottom,group.y+group.implicitHeight):bottom,0)
                    readonly property real spacing:32
                    readonly property int columnCount: Math.max(1, Math.floor(taskScroll.availableWidth / 420))
                    property var groups: []
                    property var measuredHeights: ({})
                    property var placements: ({})
                    function reset() {groups=[];placements={};balanceTimer.restart();}
                    function columnFor(group,index) {return (placements[group] ?? index)%columnCount;}
                    function topFor(index,column) {
                        let top=0;
                        for(let i=0;i<index;i++) {
                            const group=groups[i];
                            if(columnFor(group,i)===column && app.tasks(group).length>0)top+=(measuredHeights[group] || 0)+28;
                        }
                        return top;
                    }
                    function noteHeight(group,height) {
                        if(height<=0 || Math.abs((measuredHeights[group] || 0)-height)<0.5)return;
                        measuredHeights=Object.assign({},measuredHeights,{[group]:height});
                    }
                    function balance() {
                        const available=app.manualGroups();
                        if(!available.length)return;
                        if(!groups.length) {
                            const ordered=available.slice();
                            if(app.domainSort==='alphabetical')ordered.sort((a,b)=>a.localeCompare(b));
                            else if(app.domainSort==='due'){
                                const earliest=g=>app.tasks(g).filter(t=>!t.done && t.status!=='cancelled').map(t=>app.effectiveDue(t)).filter(Boolean).sort()[0] || '9999';
                                ordered.sort((a,b)=>earliest(a).localeCompare(earliest(b)) || a.localeCompare(b));
                            }
                            groups=ordered;
                        } else {
                            const added=available.filter(g=>!groups.includes(g));
                            if(added.length)groups=groups.concat(added);
                        }
                        const next=Object.assign({},placements);
                        let slot=Object.keys(next).length;
                        // Assign once; mutations and height changes never move existing domains.
                        for(const group of groups)if(next[group]===undefined && app.tasks(group).length)next[group]=slot++;
                        if(JSON.stringify(next)!==JSON.stringify(placements))placements=next;
                    }
                    onColumnCountChanged:balanceTimer.restart()
                    Timer {id:balanceTimer;interval:0;onTriggered:groupColumns.balance()}
                    Connections {target:app;function onTasksByGroupChanged(){balanceTimer.restart();}}
                    Component.onCompleted:balanceTimer.restart()

                    Repeater {
                        model:groupColumns.groups
                        ColumnLayout {
                            id:groupPanel
                            required property string modelData
                            required property int index
                            property alias rows:groupRows
                            readonly property int column:groupColumns.columnFor(modelData,index)
                            readonly property int leftCount:Math.ceil(groupColumns.columnCount/2)
                            readonly property bool rightHalf:column>=leftCount
                            readonly property int halfCount:rightHalf?groupColumns.columnCount-leftCount:leftCount
                            visible:groupRows.count>0
                            width:app.imageDetails?(groupColumns.width/2-16-groupColumns.spacing*(halfCount-1))/halfCount:(groupColumns.width-groupColumns.spacing*(groupColumns.columnCount-1))/groupColumns.columnCount
                            x:app.imageDetails?(rightHalf?groupColumns.width/2+16:0)+(column-(rightHalf?leftCount:0))*(width+groupColumns.spacing):column*(width+groupColumns.spacing)
                            y:groupColumns.topFor(index,column)
                            spacing:6
                            onImplicitHeightChanged:groupColumns.noteHeight(modelData,implicitHeight)
                            Component.onCompleted:{app.dropGroups=app.dropGroups.concat([groupPanel]);groupColumns.noteHeight(modelData,implicitHeight);}
                            Component.onDestruction:app.dropGroups=app.dropGroups.filter(g=>g!==groupPanel)
                            Timer {id:domainClick;interval:Qt.styleHints.mouseDoubleClickInterval;onTriggered:app.toggleDomain(groupPanel.modelData)}
                            RowLayout {
                                Layout.fillWidth:true;spacing:8;Layout.topMargin:6;Layout.bottomMargin:8
                                Button {
                                    objectName:'domain-collapse-'+groupPanel.modelData
                                    implicitWidth:24;implicitHeight:28;padding:0
                                    Accessible.name:(app.collapsedDomains[groupPanel.modelData]?'Expand ':'Collapse ')+groupPanel.modelData
                                    onClicked:app.toggleDomain(groupPanel.modelData)
                                    contentItem:Text {text:app.collapsedDomains[groupPanel.modelData]?'›':'⌄';color:theme.muted;font.pixelSize:18;horizontalAlignment:Text.AlignHCenter;verticalAlignment:Text.AlignVCenter}
                                    background:Rectangle {radius:4;color:parent.hovered?theme.surface:'transparent'}
                                }
                                Label {objectName:'group-heading-'+groupPanel.modelData;visible:!groupNameEdit.visible;text:modelData==='All'?'Inbox':modelData;font.pixelSize:13;font.weight:Font.Bold;font.letterSpacing:0.4;color:theme.text;MouseArea {anchors.fill:parent;onClicked:domainClick.restart();onDoubleClicked:{domainClick.stop();app.beginGroupRename(groupPanel.modelData);if(groupNameEdit.visible){groupNameEdit.text=parent.text;groupNameEdit.forceActiveFocus();groupNameEdit.selectAll();}}}}
                                TextField {
                                    id:groupNameEdit;objectName:'group-heading-edit-'+groupPanel.modelData
                                    visible:!!app.groupRename && app.groupRename.source===groupPanel.modelData
                                    Layout.fillWidth:true;implicitHeight:28;font.pixelSize:12;selectByMouse:true
                                    enabled:!app.groupRename || !app.groupRename.sent
                                    color:theme.text;padding:5
                                    background:Rectangle {radius:4;color:theme.surface;border.color:theme.accent}
                                    Accessible.name:'Rename group'
                                    onAccepted:app.saveGroupRename(text)
                                    Keys.onEscapePressed:event=>{app.groupRename=null;app.focusList();event.accepted=true;}
                                }
                                QuietButton {visible:groupNameEdit.visible;text:app.groupRename && app.groupRename.sent?'Saving…':'Save';enabled:!app.requestBusy && !!app.groupRename && !app.groupRename.sent;horizontalPadding:8;onClicked:app.saveGroupRename(groupNameEdit.text)}
                                QuietButton {visible:groupNameEdit.visible;text:'Cancel';enabled:!!app.groupRename && !app.groupRename.sent;horizontalPadding:8;onClicked:{app.groupRename=null;app.focusList();}}
                                Label {text:app.tasks(modelData).length;font.pixelSize:11;color:theme.muted}
                                Button {
                                    objectName:'domain-add-'+groupPanel.modelData
                                    visible:!groupNameEdit.visible
                                    implicitWidth:24;implicitHeight:28;padding:0
                                    enabled:!app.pendingSave && !app.pendingPurge && !app.pendingMove && !app.completing
                                    Accessible.name:'Add task to '+(groupPanel.modelData==='All'?'Inbox':groupPanel.modelData)
                                    onClicked:{app.newItem('todo',groupPanel.modelData);app.sections=Object.assign({},app.sections,{group:true});}
                                    contentItem:Text {text:'+';color:parent.hovered || parent.activeFocus?theme.accent:theme.muted;font.pixelSize:20;horizontalAlignment:Text.AlignHCenter;verticalAlignment:Text.AlignVCenter}
                                    background:Rectangle {radius:4;color:parent.hovered || parent.activeFocus?theme.surface:'transparent'}
                                    TaskToolTip {visible:parent.hovered;theme:app.theme;text:parent.Accessible.name}
                                }
                            }
                            Label {visible:!!app.groupRename && app.groupRename.source===groupPanel.modelData && !!app.groupRename.error;text:visible?app.groupRename.error:'';color:'#eb6f92';Layout.fillWidth:true;wrapMode:Text.Wrap}
                            Repeater {
                                id:groupRows
                                model:app.tasks(groupPanel.modelData)
                                Rectangle {
                                    id:taskCard
                                    visible:!app.collapsedDomains[groupPanel.modelData]
                                    required property var modelData
                                    function navigationItems(){return app.expandedTasks[modelData.id]?subtaskContent.navigationItems():[];}
                                    property real gapHeight:app.dragTask && app.dropGroup===groupPanel.modelData && app.dropBefore===modelData.id ? app.dragHeight+6 : 0
                                    Behavior on gapHeight {NumberAnimation {duration:120}}
                                    Timer {id:projectClick;interval:300;onTriggered:app.enterProject(taskCard.modelData)}
                                    objectName:'task-'+modelData.id
                                    opacity:app.dragTask && app.dragTask.id===modelData.id ? 0.5 : 1
                                    border.width:app.dropProject===modelData.id?1:0
                                    Layout.fillWidth:true;implicitHeight:cardBody.implicitHeight+8+gapHeight+snoozeReveal.height+subtaskReveal.height;radius:4
                                    color:"transparent"
                                    border.color:app.dropProject===modelData.id?theme.accent:"transparent"
                                    Item {
                                        id:snoozeReveal
                                        anchors.top:cardBody.bottom;anchors.left:parent.left;anchors.right:parent.right
                                        anchors.leftMargin:42;anchors.topMargin:8
                                        height:app.snoozeTaskId===taskCard.modelData.id?snoozeContent.implicitHeight+12:0
                                        visible:height>0;clip:true;z:3
                                        Behavior on height {NumberAnimation {duration:180;easing.type:Easing.OutCubic}}
                                        ColumnLayout {
                                            id:snoozeContent;width:parent.width;spacing:6
                                            SnoozePicker {
                                                Layout.fillWidth:true;ink:theme.text;accent:theme.accent
                                                opened:app.snoozeTaskId===taskCard.modelData.id
                                                onChosen:until=>{app.action('snooze',{until:until});app.snoozeTaskId='';app.focusList();}
                                                onCancelled:{app.snoozeTaskId='';app.focusList();}
                                            }
                                        }
                                    }
                                    Item {
                                        id:subtaskReveal;z:3;clip:true
                                        anchors.top:snoozeReveal.bottom;anchors.left:parent.left;anchors.right:parent.right;anchors.leftMargin:4;anchors.rightMargin:4
                                        visible:height>0
                                        height:app.expandedTasks[taskCard.modelData.id]?subtaskContent.implicitHeight:0
                                        Behavior on height {NumberAnimation {duration:160;easing.type:Easing.InOutCubic}}
                                        SubtaskList {id:subtaskContent;width:parent.width;enabled:!!app.expandedTasks[taskCard.modelData.id]
                                        task:taskCard.modelData;theme:app.theme;busy:!!app.pendingSave || !!app.pendingPurge || app.pendingMove;showBlocked:app.showBlocked;view:app.view;today:app.listState.today;focusedId:app.taskSelectionActive && app.selectedId===taskCard.modelData.id?app.selectedSubtaskId:''
                                        onToggle:id=>{app.selectedSubtaskId=id;app.selectedId=taskCard.modelData.id;app.toggleCompletion(taskCard.modelData.id,id);}
                                        onNavigate:(direction,modifiers)=>app.focusVertical(direction,modifiers)
                                        onFocusReached:id=>app.focusRow(app.navigationRows().find(r=>r.id===taskCard.modelData.id && r.subtaskId===id))
                                        onAdd:app.addSubtask(taskCard.modelData)
                                        }
                                    }
                                    RowLayout {
                                        id:cardBody
                                        z:1
                                        anchors.left:parent.left;anchors.right:parent.right;anchors.top:parent.top;anchors.margins:4;anchors.topMargin:4+taskCard.gapHeight;spacing:8
                                        Item {
                                            id:taskMarker
                                            readonly property bool focusedTask: app.taskSelectionActive && selectedId===modelData.id && !app.selectedSubtaskId
                                            readonly property bool showCheck: taskCard.modelData.kind==='todo' && (cardHover.hovered || completionButton.activeFocus)
                                            Layout.minimumWidth:30;Layout.preferredWidth:30;Layout.maximumWidth:30;Layout.preferredHeight:taskTitle.firstLineHeight;Layout.alignment:Qt.AlignTop
                                            Canvas {
                                                id:completionMarker;objectName:'completion-marker-'+taskCard.modelData.id
                                                visible:taskMarker.focusedTask || morph>0
                                                width:18;height:18;anchors.centerIn:parent
                                                property real morph:taskMarker.showCheck?1:0
                                                Behavior on morph {NumberAnimation {duration:160;easing.type:Easing.InOutCubic}}
                                                property color ink:theme.accent
                                                property color checkInk:theme.background
                                                property bool done:taskCard.modelData.done
                                                onMorphChanged:requestPaint()
                                                onInkChanged:requestPaint()
                                                onCheckInkChanged:requestPaint()
                                                onDoneChanged:requestPaint()
                                                onPaint:{
                                                    const c=getContext('2d'),t=morph;c.reset();
                                                    c.globalAlpha=taskMarker.focusedTask?1:t;
                                                    c.strokeStyle=ink;c.lineWidth=1.5;c.lineCap='round';c.lineJoin='round';
                                                    // One centered stroke unfolds from a chevron into the box.
                                                    const chevron=[[7,5],[9,7],[11,9],[9,11],[7,13]];
                                                    const box=[[2,2],[16,2],[16,16],[2,16],[2,2]];
                                                    c.beginPath();
                                                    for(let i=0;i<box.length;i++) {
                                                        const x=chevron[i][0]+(box[i][0]-chevron[i][0])*t;
                                                        const y=chevron[i][1]+(box[i][1]-chevron[i][1])*t;
                                                        if(i===0)c.moveTo(x,y);else c.lineTo(x,y);
                                                    }
                                                    if(done){c.fillStyle=ink;c.globalAlpha=t;c.fill();c.globalAlpha=taskMarker.focusedTask?1:t;}
                                                    c.stroke();
                                                    if(done && t>0){c.globalAlpha=t;c.strokeStyle=checkInk;c.beginPath();c.moveTo(5,9);c.lineTo(8,12);c.lineTo(13,6);c.stroke();}
                                                }
                                                Connections {target:taskMarker;function onFocusedTaskChanged(){completionMarker.requestPaint();}}
                                            }
                                            Label {
                                                objectName:'task-date-'+taskCard.modelData.id
                                                visible:!parent.focusedTask && !taskMarker.showCheck
                                                width:parent.width;height:parent.height
                                                horizontalAlignment:Text.AlignHCenter;verticalAlignment:Text.AlignVCenter
                                                text:app.dueLabel(modelData)
                                                color:app.isOverdue(modelData)?'#eb6f92':theme.muted
                                                font.family:'monospace';font.pixelSize:11;font.weight:Font.DemiBold
                                                font.underline:!!modelData.snoozedUntil
                                            }
                                        }
                                        ColumnLayout {
                                            Layout.fillWidth:true;spacing:3
                                            Label {
                                                id: taskTitle;objectName:'task-title-'+taskCard.modelData.id
                                                Layout.fillWidth:true
                                                Layout.rightMargin:Math.max(24,statusIcons.visible?statusIcons.implicitWidth+6:0)
                                                topPadding:2;bottomPadding:2
                                                property real lastLineWidth: 0
                                                property real lastLineTop: 0
                                                property real firstLineHeight: 18
                                                text:app.socialTitle(modelData)
                                                leftPadding:socialIcons.visible?socialIcons.width+6:0
                                                textFormat:Text.PlainText
                                                font.pixelSize:15
                                                font.weight:Font.DemiBold
                                                font.strikeout:modelData.done && modelData.status!=='cancelled'
                                                color:taskSelectionActive && selectedId===modelData.id && !selectedSubtaskId?theme.accent:theme.text
                                                wrapMode:Text.Wrap;maximumLineCount:3;elide:Text.ElideRight
                                                property var strikeLines: []
                                                onLineLaidOut: line => {
                                                    if(line.number===0) {strikeLines=[];firstLineHeight=line.height;}
                                                    strikeLines.push({x:line.x,y:line.y+line.height*0.52,width:Math.min(line.width,line.implicitWidth)});
                                                    lastLineWidth=Math.min(line.width,line.implicitWidth);
                                                    lastLineTop=line.y;
                                                    if(cancelledStrike.visible)cancelledStrike.requestPaint();
                                                }
                                                onColorChanged: if(cancelledStrike.visible)cancelledStrike.requestPaint()
                                                Row {
                                                    id:socialIcons;objectName:'social-icons-'+taskCard.modelData.id
                                                    y:taskTitle.topPadding+(taskTitle.firstLineHeight-16)/2;spacing:4
                                                    visible:socialIconRepeater.count>0
                                                    Repeater {
                                                        id:socialIconRepeater;model:app.socialTags(taskCard.modelData)
                                                        Canvas {
                                                            required property string modelData
                                                            width:16;height:16
                                                            Accessible.name:modelData==='hangout'?'Hangout':modelData==='message'?'Message':'Call'
                                                            property color ink:taskTitle.color
                                                            onInkChanged:requestPaint()
                                                            onPaint:{
                                                                const c=getContext('2d');c.reset();c.strokeStyle=ink;c.lineWidth=1.3;c.lineCap='round';c.lineJoin='round';c.beginPath();
                                                                if(modelData==='message'){
                                                                    c.moveTo(2,2.5);c.lineTo(14,2.5);c.lineTo(14,11);c.lineTo(7,11);c.lineTo(3,14);c.lineTo(3,11);c.lineTo(2,11);c.closePath();
                                                                    c.moveTo(5,5.5);c.lineTo(11,5.5);c.moveTo(5,8);c.lineTo(9,8);
                                                                }else if(modelData==='hangout'){
                                                                    c.arc(5.5,5,2.3,0,Math.PI*2);c.moveTo(1,14);c.bezierCurveTo(1,7.5,10,7.5,10,14);
                                                                    c.moveTo(10,2.8);c.bezierCurveTo(14,2.8,14,7,10,7);c.moveTo(11,9);c.bezierCurveTo(14,9,15,11,15,14);
                                                                }else{
                                                                    c.moveTo(3,1.5);c.lineTo(6,1.5);c.lineTo(7,5);c.lineTo(5,6.5);c.bezierCurveTo(6,9,7,10,9.5,11);c.lineTo(11,9);c.lineTo(14.5,10);c.lineTo(14.5,13);c.quadraticCurveTo(14,15,11,14);c.bezierCurveTo(6,13,3,10,2,5);c.quadraticCurveTo(1,2,3,1.5);
                                                                }
                                                                c.stroke();
                                                            }
                                                        }
                                                    }
                                                }
                                                Row {
                                                    id:statusIcons
                                                    x:taskTitle.leftPadding+taskTitle.lastLineWidth+6;y:taskTitle.lastLineTop+2
                                                    spacing:5
                                                    visible:blockedIcon.visible || repeatIcon.visible || imageIcon.visible
                                                    Canvas {
                                                        id:imageIcon;width:14;height:14
                                                        visible:(modelData.images || []).length>0
                                                        Accessible.name:'Has images'
                                                        onPaint:{const c=getContext('2d');c.reset();c.strokeStyle=theme.muted;c.lineWidth=1.2;c.strokeRect(1,2,12,10);c.beginPath();c.arc(9.5,5,1,0,Math.PI*2);c.stroke();c.beginPath();c.moveTo(2,11);c.lineTo(5,7);c.lineTo(8,10);c.lineTo(10,8);c.lineTo(12,11);c.stroke();}
                                                    }
                                                    Canvas {
                                                        id:blockedIcon;width:14;height:14
                                                        visible:modelData.status==='blocked'
                                                        Accessible.name:'Blocked'
                                                        onPaint:{const c=getContext('2d');c.reset();c.strokeStyle='#f6c177';c.lineWidth=1.5;c.beginPath();c.arc(7,7,5.3,0,Math.PI*2);c.moveTo(3.3,10.7);c.lineTo(10.7,3.3);c.stroke();}
                                                    }
                                                    Canvas {
                                                        id:repeatIcon;width:14;height:14
                                                        visible:!!modelData.repeat && !!modelData.repeat.unit
                                                        Accessible.name:'Repeats'
                                                        onPaint:{const c=getContext('2d');c.reset();c.strokeStyle=theme.muted;c.lineWidth=1.4;c.lineCap='round';c.beginPath();c.arc(7,7,4.6,Math.PI*1.1,Math.PI*1.9);c.moveTo(11.4,2.4);c.lineTo(11.4,5.4);c.lineTo(8.4,5.4);c.moveTo(2.6,11.6);c.lineTo(2.6,8.6);c.lineTo(5.6,8.6);c.moveTo(11.4,8.4);c.arc(7,7,4.6,Math.PI*0.1,Math.PI*0.9);c.stroke();}
                                                    }
                                                }
                                                Canvas {
                                                    id: cancelledStrike
                                                    anchors.fill:parent
                                                    visible:modelData.status==='cancelled'
                                                    onVisibleChanged:if(visible)requestPaint()
                                                    onWidthChanged:if(visible)requestPaint()
                                                    onHeightChanged:if(visible)requestPaint()
                                                    onPaint: {
                                                        const ctx=getContext('2d');ctx.reset();
                                                        ctx.strokeStyle=taskTitle.color;ctx.lineWidth=1.2;
                                                        for(const line of taskTitle.strikeLines) {
                                                            ctx.beginPath();
                                                            for(let x=0;x<=line.width;x++) {
                                                                const y=line.y+1.6*Math.sin(x*Math.PI/5);
                                                                if(x===0)ctx.moveTo(line.x+x,y);else ctx.lineTo(line.x+x,y);
                                                            }
                                                            ctx.stroke();
                                                        }
                                                    }
                                                }
                                            }
                                            ProgressBar {
                                                id:projectProgressBar
                                                visible:taskCard.modelData.kind==='project'
                                                Layout.preferredWidth:parent.width*0.7;Layout.minimumWidth:0;Layout.preferredHeight:4;Layout.alignment:Qt.AlignLeft
                                                Layout.topMargin:5;Layout.bottomMargin:5
                                                padding:0
                                                from:0;to:Math.max(1,app.projectProgress(taskCard.modelData).total)
                                                value:app.projectProgress(taskCard.modelData).done
                                                Accessible.name:'Project progress'
                                                background:Rectangle {radius:2;color:theme.surface}
                                                contentItem:Item {
                                                    Rectangle {width:parent.width*projectProgressBar.position;height:parent.height;radius:2;color:theme.accent}
                                                }
                                            }
                                            Button {
                                                id:pipButton;objectName:'subtask-pips-'+taskCard.modelData.id
                                                visible:(taskCard.modelData.subtasks || []).length>0
                                                Layout.fillWidth:true;padding:0;topPadding:4;bottomPadding:4
                                                Accessible.name:'Subtasks: '+(taskCard.modelData.subtaskProgress?.done || 0)+' of '+(taskCard.modelData.subtaskProgress?.total || 0)+' complete'
                                                TaskToolTip {theme:app.theme;visible:pipButton.hovered;text:pipButton.Accessible.name}
                                                onClicked:app.toggleExpanded(taskCard.modelData.id)
                                                background:Item {}
                                                contentItem:Flow {
                                                    spacing:5
                                                    Repeater {
                                                        model:taskCard.modelData.subtaskPips || []
                                                        Rectangle {
                                                            required property var modelData
                                                            width:6;height:6;radius:3
                                                            color:modelData.status==='done'?theme.accent:modelData.status==='cancelled'?'#eb6f92':'transparent'
                                                            border.width:1;border.color:modelData.status==='done'?theme.accent:theme.muted
                                                            opacity:modelData.status==='blocked' || modelData.status==='cancelled'?0.4:1
                                                        }
                                                    }
                                                }
                                            }
                                            Label {Layout.fillWidth:true;text:taskMeta(modelData);visible:text!=='';color:theme.muted;font.pixelSize:12;wrapMode:Text.Wrap}
                                            Flow {
                                                id:tagFlow
                                                Layout.fillWidth:true;Layout.minimumWidth:0
                                                spacing:8;visible:(taskCard.modelData.tags || []).length>0
                                                Repeater {
                                                    model:taskCard.modelData.tags || []
                                                    Label {
                                                        required property string modelData
                                                        text:'#'+modelData;textFormat:Text.PlainText
                                                        width:Math.min(implicitWidth,tagFlow.width)
                                                        font.pixelSize:11;color:tagMouse.containsMouse?theme.text:theme.accent
                                                        wrapMode:Text.WrapAnywhere
                                                        Accessible.name:'Filter by tag '+modelData
                                                        MouseArea {
                                                            id:tagMouse;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor
                                                            onClicked:app.filterTag(parent.modelData)
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    AddSubtaskButton {
                                        theme:app.theme
                                        z:3;anchors.right:parent.right;anchors.top:parent.top;width:28;height:28
                                        visible:taskCard.modelData.kind==='todo' && (taskCard.modelData.subtasks || []).length===0 && (cardHover.hovered || activeFocus)
                                        onClicked:app.addSubtask(taskCard.modelData)
                                    }
                                    Button {
                                        id:completionButton;objectName:'complete-'+taskCard.modelData.id
                                        z:4;x:4;y:4+taskCard.gapHeight;width:30;height:Math.max(26,taskTitle.firstLineHeight)
                                        visible:taskCard.modelData.kind==='todo';focusPolicy:Qt.NoFocus
                                        enabled:!app.pendingSave && !app.pendingPurge && !app.pendingMove
                                        Accessible.name:taskCard.modelData.done?'Reopen task':'Complete task'
                                        background:Item {}
                                        contentItem:Item {}
                                        onClicked:app.toggleCompletion(taskCard.modelData.id,'')
                                    }
                                    HoverHandler {id:cardHover}
                                    MouseArea {
                                        anchors.fill:parent;acceptedButtons:Qt.LeftButton
                                        cursorShape:pressed?Qt.ClosedHandCursor:Qt.OpenHandCursor
                                        preventStealing:true
                                        property point pressedPoint
                                        property bool moved: false
                                        onPressed: mouse=>{pressedPoint=mapToItem(app,mouse.x,mouse.y);moved=false;selectedId=modelData.id;app.focusList();}
                                        onPositionChanged: mouse=>{
                                            if(!pressed || app.pendingMove)return;
                                            const point=mapToItem(app,mouse.x,mouse.y);
                                            if(!moved && Math.hypot(point.x-pressedPoint.x,point.y-pressedPoint.y)>10){moved=true;app.startDrag(modelData,cardBody,taskCard.width,cardBody.implicitHeight+8,point);}
                                            if(moved && app.dragTask)app.updateDrag(point);
                                        }
                                        onReleased: {if(moved)app.finishDrag();}
                                        onCanceled: {if(moved && !app.pendingMove)app.cancelDrag();}
                                        onClicked:{if(!moved){selectedId=modelData.id;app.focusList();if(modelData.kind==='project')projectClick.start();else if((modelData.subtasks || []).length)app.toggleExpanded(modelData.id);}}
                                        onDoubleClicked:{if(!moved){projectClick.stop();selectedId=modelData.id;app.openEditor(modelData);}}
                                    }
                                }
                            }
                            Item {
                                Layout.fillWidth:true
                                implicitHeight:app.dragTask && app.dropGroup===groupPanel.modelData && app.dropBefore===''?app.dragHeight+6:groupRows.count===0?30:0
                                Behavior on implicitHeight {NumberAnimation {duration:120}}
                                Label {visible:groupRows.count===0 && !!app.dragTask;anchors.centerIn:parent;text:'Drop tasks here';color:theme.muted;font.pixelSize:11}
                            }
                        }
                    }
                }
                ColumnLayout {
                    objectName:'empty-state';visible:visibleCount()===0;Layout.fillWidth:true;Layout.preferredHeight:Math.max(0,taskScroll.availableHeight-14);spacing:10
                    Item {Layout.fillHeight:true}
                    Label {text:searching && filter?'No matching tasks':'Nothing scheduled here';color:theme.text;font.pixelSize:18;Layout.alignment:Qt.AlignHCenter}
                    Label {text:view==='all'?'Add a task to get started.':'Check All tasks or change your filters.';color:theme.muted;font.pixelSize:13;wrapMode:Text.Wrap;Layout.fillWidth:true;horizontalAlignment:Text.AlignHCenter}
                    Item {Layout.fillHeight:true}
                }
            }
        }
        RowLayout {
            visible:!editor;Layout.fillWidth:true;spacing:4
            IconButton {objectName:'edit-task';symbol:'edit';description:'Edit focused task (E)';enabled:!!selected;onClicked:openEditor()}
            Item {Layout.fillWidth:true}
            IconButton {symbol:'undo';description:'Undo (U / Ctrl+Z)';onClicked:app.undoLastAction()}
            IconButton {visible:collection!=='reading';symbol:'info';description:'Due dates and snoozing';onClicked:dateHelp.open()}
            IconButton {symbol:'keyboard';description:'Keyboard shortcuts (?)';onClicked:help.open()}
            IconButton {objectName:'settings-button';symbol:'settings';description:'Settings: groups and templates';onClicked:taskSettings.open()}
        }
        KineticScrollView {
            id:editorScroll;objectName:'editor-scroll';visible:editor==='edit';enabled:visible;Layout.fillWidth:true;Layout.fillHeight:true;clip:true;contentWidth:availableWidth;contentHeight:editorForm.implicitHeight;ScrollBar.horizontal.policy:ScrollBar.AlwaysOff
            ScrollBar.vertical: EdgeScrollBar {scrollView:editorScroll}
            ColumnLayout {
                id:editorForm
                width:editorScroll.availableWidth;height:implicitHeight;spacing:18
                ColumnLayout {
                    Layout.fillWidth:true;spacing:6
                    Label {text:'Title';color:theme.muted;font.pixelSize:12}
                    Entry {id:titleInput;Layout.fillWidth:true;placeholderText:collection==='reading'?'Page title':'What needs to be done?';font.pixelSize:18;onAccepted:if(text.trim())saveEditor()}
                }
                Flow {
                    id:detailButtons
                    Layout.fillWidth:true;Layout.preferredHeight:implicitHeight;Layout.maximumHeight:implicitHeight;spacing:6
                    SearchChoice {
                        id:templatePicker;objectName:'from-template';label:'Template'
                        visible:!!editing && !editing.id && editing.kind==='todo'
                        model:[{id:'',title:'From Template…'}].concat((stateData.tasks || []).filter(t=>t.kind==='template'))
                        onActivated:{const template=model[currentIndex];if(template.id){const copy=Workflow.instantiate(template.subtasks || []);workflowEditor.reset(copy,template.workflow===true);app.reveal('subtasks');}}
                    }
                    SearchChoice {
                        id:projectPicker;objectName:'project-choice';label:'Project';allowEmpty:true
                        visible:!!editing && editing.kind==='todo'
                        model:[{id:'',title:'No project'}].concat((stateData.tasks || []).filter(t=>t.kind==='project')).concat(editing && editing.project && !(stateData.tasks || []).some(t=>t.kind==='project' && t.id===editing.project)?[{id:editing.project,title:editing.project+' (legacy)'}]:[])
                    }
                    QuietButton {
                        objectName:'add-subtasks';text:'+ Subtasks';implicitHeight:34;horizontalPadding:12
                        visible:!!editing && editing.kind==='todo' && !workflowEditor.visible
                        onClicked:app.reveal('subtasks')
                    }
                    Repeater {
                        model:[{label:'Domain',key:'group'},{label:'Tags',key:'tags'},{label:'Due date',key:'due'},{label:'Repeat',key:'repeat'},{label:'Note',key:'reference'},{label:'Reminder',key:'reminder'}]
                        QuietButton {required property var modelData;objectName:'add-'+modelData.key;text:'+ '+modelData.label;visible:!sections[modelData.key];implicitHeight:34;horizontalPadding:12;onClicked:reveal(modelData.key)}
                    }
                }
                CheckBox {id:projectOnly;text:'Only show Project tasks in their project';visible:!!editing && editing.kind==='project';checked:true}
                WorkflowEditor {id:workflowEditor;Layout.fillWidth:true;theme:app.theme;visible:!!editing && (editing.kind==='template' || editing.kind==='todo' && (!!sections.subtasks || nodes.length>0 || sequential))}
                ColumnLayout {
                    visible:!!sections.reference;Layout.fillWidth:true;spacing:6
                    Label {text:editing && editing.kind==='reading'?'Link':'Reference';color:theme.muted;font.pixelSize:12}
                    RowLayout {
                        Layout.fillWidth:true;spacing:8
                        Entry {id:planInput;Layout.fillWidth:true;placeholderText:editing && editing.kind==='reading'?'https://…':'Obsidian vault path'}
                        QuietButton {text:'Find note';visible:!editing || editing.kind!=='reading';onClicked:{request({action:'plans'});planPicker.visible=true;}}
                    }
                    ColumnLayout {
                        id:planPicker;visible:false;Layout.fillWidth:true
                        Entry {id:planFilter;placeholderText:'Fuzzy file search';Layout.fillWidth:true}
                        Repeater {
                            model:plans.filter(p=>{let i=0;for(const c of p.toLowerCase())if(c===planFilter.text.toLowerCase()[i])i++;return i===planFilter.text.length;}).slice(0,15)
                            QuietButton {required property string modelData;text:modelData;Layout.fillWidth:true;onClicked:{planInput.text=modelData;planPicker.visible=false;}}
                        }
                    }
                }
                ColumnLayout {
                    visible:!!sections.group;Layout.fillWidth:true;spacing:6
                    Label {text:'Domain';color:theme.muted;font.pixelSize:12}
                    Choice {id:groupInput;Layout.fillWidth:true;editable:true;model:stateData.groups || [];inputMethodHints:Qt.ImhNoPredictiveText}
                }
                ColumnLayout {
                    visible:!!sections.tags;Layout.fillWidth:true;spacing:6
                    Label {text:'Tags';color:theme.muted;font.pixelSize:12}
                    Entry {id:tagsInput;Layout.fillWidth:true;placeholderText:'Comma separated'}
                }
                ColumnLayout {
                    visible:!!sections.due;Layout.fillWidth:true;spacing:6
                    Label {text:'Due date';color:theme.muted;font.pixelSize:12}
                    Entry {
                        id:dueInput;Layout.fillWidth:true;placeholderText:'Tomorrow, next Friday, or YYYY-MM-DD';Accessible.name:'Due date'
                        onTextChanged:{dueSuggestionsDismissed=false;dueSuggestionIndex=text.trim() && DueDate.suggestions(text,dueToday).length?0:-1;}
                        onActiveFocusChanged:{if(activeFocus)dueSuggestionsDismissed=false;}
                        onAccepted:{if(showDueSuggestions && dueSuggestionIndex>=0)acceptDueSuggestion(dueSuggestionIndex);else saveEditor();}
                        Keys.onDownPressed:event=>{
                            if(dueSuggestions.length){dueSuggestionsDismissed=false;dueSuggestionIndex=(dueSuggestionIndex+1)%dueSuggestions.length;event.accepted=true;}
                            else event.accepted=false;
                        }
                        Keys.onUpPressed:event=>{
                            if(dueSuggestions.length){dueSuggestionsDismissed=false;dueSuggestionIndex=(dueSuggestionIndex<=0?dueSuggestions.length:dueSuggestionIndex)-1;event.accepted=true;}
                            else event.accepted=false;
                        }
                        Keys.onTabPressed:event=>{
                            if(showDueSuggestions && dueSuggestionIndex>=0){acceptDueSuggestion(dueSuggestionIndex);event.accepted=true;}
                            else event.accepted=false;
                        }
                        Keys.onEscapePressed:event=>{
                            if(showDueSuggestions){dueSuggestionsDismissed=true;event.accepted=true;}
                            else event.accepted=false;
                        }
                    }
                    ColumnLayout {
                        id:dueSuggestionList
                        visible:showDueSuggestions;Layout.fillWidth:true;spacing:2
                        Repeater {
                            model:dueSuggestions
                            ItemDelegate {
                                required property var modelData
                                required property int index
                                objectName:'due-suggestion-'+index
                                Layout.fillWidth:true;implicitHeight:38;focusPolicy:Qt.NoFocus
                                Accessible.name:modelData.label+', '+(modelData.value || 'No due date')
                                background:Rectangle {radius:6;color:index===dueSuggestionIndex?Qt.alpha(theme.accent,0.16):parent.hovered?theme.surface:'transparent';border.color:index===dueSuggestionIndex?theme.accent:'transparent'}
                                contentItem:RowLayout {
                                    spacing:10
                                    Label {text:modelData.label;color:theme.text;Layout.fillWidth:true;elide:Text.ElideRight;font.pixelSize:13}
                                    Label {text:modelData.value?Qt.formatDate(new Date(modelData.value+'T12:00:00'),'ddd, d MMM'):'No due date';color:theme.muted;font.pixelSize:12}
                                }
                                onClicked:acceptDueSuggestion(index)
                            }
                        }
                    }
                    Label {
                        id:duePreview
                        Layout.fillWidth:true;wrapMode:Text.Wrap;font.pixelSize:12
                        text:showDueSuggestions?'↑ ↓ choose · Enter or Tab to use · Esc to dismiss':parsedDue.error || (parsedDue.value?Qt.formatDate(new Date(parsedDue.value+'T12:00:00'),'dddd, d MMMM yyyy')+' · '+parsedDue.value:'Try “in 2 weeks” or “15 December”. Leave blank for no due date.')
                        color:parsedDue.error && !showDueSuggestions?'#eb6f92':theme.muted
                    }
                }
                ColumnLayout {
                    visible:!!sections.repeat;Layout.fillWidth:true;spacing:6
                    Label {text:'Repeat';color:theme.muted;font.pixelSize:12}
                    RowLayout {
                        Layout.fillWidth:true;spacing:8
                        Entry {
                            id:repeatInput;objectName:'repeat-input';visible:app.repeatTyping;Layout.fillWidth:true
                            placeholderText:'Every 2 weeks, every Monday, or the 1st of every month';Accessible.name:'Repeat schedule'
                            onAccepted:{if(!app.parsedRepeat.error){app.repeatTyping=false;repeatEdit.forceActiveFocus();}}
                            onEditingFinished:if(!app.parsedRepeat.error)app.repeatTyping=false
                        }
                        Label {visible:!app.repeatTyping;Layout.fillWidth:true;text:app.parsedRepeat.label;color:theme.text;wrapMode:Text.Wrap}
                        IconButton {id:repeatEdit;objectName:'repeat-edit';visible:!app.repeatTyping;symbol:'edit';description:'Retype repeat schedule';onClicked:{app.repeatTyping=true;repeatInput.forceActiveFocus();repeatInput.selectAll();}}
                    }
                    Label {
                        Layout.fillWidth:true;wrapMode:Text.Wrap;font.pixelSize:12
                        text:app.parsedRepeat.error || (app.repeatTyping?app.parsedRepeat.label:'')
                        color:app.parsedRepeat.error?'#eb6f92':theme.muted
                    }
                }
                RowLayout {
                    visible:!!sections.reminder
                    CheckBox {id:notifyInput;text:'Remind before due';checked:true}
                    Entry {id:leadValue;Layout.preferredWidth:60;text:'1';validator:IntValidator {bottom:0;top:10000}}
                    Choice {id:leadUnit;model:['hours','days','weeks'];currentIndex:1}
                }
                RowLayout {
                    id:editorActions
                    Layout.topMargin:6;spacing:8
                    QuietButton {primary:true;text:pendingSave?'Saving…':draftId?'Save and close tab':'Save';enabled:!pendingSave && titleInput.text.trim()!=='';onClicked:saveEditor()}
                    QuietButton {text:'Close';enabled:!pendingSave;onClicked:cancelEditor()}
                    QuietButton {objectName:'cancel-task';text:editing && editing.status==='cancelled'?'Restore task (C)':'Cancel task (C)';visible:!!editing && !!editing.id && editing.kind==='todo';enabled:!requestBusy && !pendingSave;onClicked:app.cancelTask()}
                }
                ColumnLayout {
                    id:metadataFooter
                    objectName:'task-metadata'
                    Layout.fillWidth:true;Layout.topMargin:10;spacing:10
                    readonly property var metadata:editing && editing.metadata || ({})
                    readonly property var keys:Object.keys(metadata).sort()
                    visible:!!editing && !!editing.id && keys.length>0
                    Accessible.name:'Task metadata'
                    Rectangle {Layout.fillWidth:true;implicitHeight:1;color:theme.border}
                    Flow {
                        id:metadataFlow
                        Layout.fillWidth:true;spacing:8
                        Repeater {
                            model:metadataFooter.keys
                            delegate:Rectangle {
                                id:metadataPair
                                required property string modelData
                                objectName:'metadata-pair-'+modelData
                                readonly property var value:metadataFooter.metadata[modelData]
                                readonly property string displayValue:value===''?'-':typeof value==='string'?value:JSON.stringify(value)
                                width:Math.min(metadataFlow.width,Math.ceil(keyMetrics.advanceWidth)+Math.ceil(valueMetrics.boundingRect.width)+26)
                                height:pairContent.implicitHeight+10
                                radius:5;color:Qt.alpha(theme.surface,0.6)
                                TextMetrics {id:keyMetrics;font:metadataKey.font;text:metadataKey.text}
                                TextMetrics {id:valueMetrics;font:metadataValue.font;text:metadataValue.text}
                                Row {
                                    id:pairContent
                                    x:8;y:5;spacing:6
                                    Text {
                                        id:metadataKey
                                        text:metadataPair.modelData+' :';textFormat:Text.PlainText
                                        font.family:'monospace';font.pixelSize:11;color:theme.muted
                                        width:Math.min(Math.ceil(keyMetrics.advanceWidth),Math.max(0,metadataPair.width-22-Math.min(24,Math.ceil(valueMetrics.boundingRect.width))));wrapMode:Text.WrapAnywhere
                                    }
                                    Text {
                                        id:metadataValue
                                        objectName:'metadata-value-'+metadataPair.modelData
                                        text:metadataPair.displayValue;textFormat:Text.PlainText
                                        font.family:'monospace';font.pixelSize:11;color:theme.text
                                        width:Math.max(0,metadataPair.width-metadataKey.width-22);wrapMode:Text.WrapAnywhere
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Label {visible:!!pendingSave;text:'Saving “'+(pendingSave?pendingSave.draft.title:'')+'”…';color:theme.accent;wrapMode:Text.Wrap;Layout.fillWidth:true}
        Label {visible:!!stateData.sync_message;text:stateData.sync_message || '';color:theme.accent;wrapMode:Text.Wrap;Layout.fillWidth:true}
        Label {visible:!!errorMessage || !!completionMessage || !!moveMessage || !!stateData.message;text:errorMessage || completionMessage || moveMessage || stateData.message || '';color:theme.accent;wrapMode:Text.Wrap;Layout.fillWidth:true}
    }
    function accepted() {
        if(completionQueue.length && completionQueue[0].sent){acceptCompletion();return;}
        if(pendingSave) {
            if(!pendingSave.sent)return;
            const value=pendingSave.request;
            const saved=(stateData.tasks || []).find(t=>value.id?t.id===value.id:t.createKey===value.request_id);
            if(saved){selectedId=saved.id;if(saved.kind==='template'){taskSettings.tab='templates';taskSettings.open();}else if(saved.kind==='project'){collection='todo';projectId='';view='projects';}}
            pendingSave=null;retrySave=null;
        }
        draftId='';editor='';app.focusList();
    }
}
