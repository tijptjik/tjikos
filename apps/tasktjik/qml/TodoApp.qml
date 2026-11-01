import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import "DueDate.js" as DueDate

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
    property var stateData: ({tasks:[],groups:[]})
    property var theme
    property string collection: 'todo'
    property string draftId: ''
    property string lastDraftId: ''
    property string view: 'week'
    property bool searching: false
    property bool showCompleted: false
    property bool showCancelled: false
    property bool showDetails: false
    property bool readingGrid: false
    property bool readingCategories: false
    property int readingSort: 0
    property int sortOrder: 0
    property bool requestBusy: false
    property var pendingSave: null
    property var retrySave: null
    property var dueToday: new Date()
    readonly property var parsedDue: DueDate.parse(dueInput.text, dueToday)
    readonly property var dueSuggestions: DueDate.suggestions(dueInput.text, dueToday)
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
            Object.assign({},task,{group:moveOperation.group},moveOperation.group==='Quick'?{due:state.today || Qt.formatDate(new Date(),'yyyy-MM-dd')}:{}) : task);
        for(const operation of (pendingPurge?[pendingPurge]:[]).concat(queuedPurges)) {
            if(operation.undoRequested) {
                if(!tasks.some(t=>t.id===operation.id))tasks.push(operation.task);
            } else tasks=tasks.filter(t=>t.id!==operation.id);
        }
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
    property bool pendingMove: false
    property bool moveSent: false
    property var moveOperation: null
    property string moveMessage: ''
    onSortOrderChanged: {if(dragTask && !pendingMove)cancelDrag();}
    function cancelDrag() {
        dropGroup='';dropBefore='';dragTask=null;dragSource=null;
    }
    function clearMove() {
        moveTimeout.stop();pendingMove=false;moveSent=false;moveOperation=null;
    }
    function rejectMove() {
        const id=moveOperation.id;
        clearMove();selectedId=id;selectVisibleTask();
        moveMessage='Could not move task: '+errorMessage;
    }
    function startDrag(task, source, width, height, point) {
        if(pendingMove || pendingSave || pendingPurge)return;
        dragSnapshot=stateData;dragSource=source;dragWidth=width;dragHeight=height;
        dragTask=JSON.parse(JSON.stringify(task));moveMessage='';selectedId=task.id;
        updateDrag(point);
    }
    function updateDrag(point) {
        dragPoint=point;
        const viewport=taskScroll.mapFromItem(app,point.x,point.y);
        if(viewport.x<0 || viewport.y<0 || viewport.x>taskScroll.width || viewport.y>taskScroll.height){dropGroup='';dropBefore='';return;}
        for(const group of dropGroups) {
            const local=group.mapFromItem(app,point.x,point.y);
            if(local.x<0 || local.x>group.width || local.y<0 || local.y>group.height)continue;
            let before='';
            for(let i=0;i<group.rows.count;i++) {
                const row=group.rows.itemAt(i);
                if(row.modelData.id===dragTask.id)continue;
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
        pendingMove=true;
        cancelDrag();
        moveTimeout.restart();sendMove();
    }
    function sendMove() {
        if(!pendingMove || moveSent || requestBusy)return;
        moveSent=true;
        request(Object.assign({action:'move'},moveOperation));
    }
    Timer {interval:50;repeat:true;running:app.pendingMove && !app.moveSent;onTriggered:app.sendMove()}
    Timer {id:moveTimeout;interval:15000;onTriggered:{app.clearMove();app.moveMessage='Move was not confirmed; refreshing tasks.';app.request({action:'list'});}}
    Timer {
        interval:30;repeat:true;running:!!app.dragTask && !app.pendingMove
        onTriggered:{
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
    onShowCompletedChanged: selectVisibleTask()
    onShowCancelledChanged: selectVisibleTask()
    property string filter: ''
    property string selectedId: ''
    property var editing: null
    property string editor: ''
    property string snoozeTaskId: ''
    onSelectedIdChanged: snoozeTaskId=''
    function orderedGroups() {
        return (listState?.groups || []).slice().sort((a,b)=>Number(b==='All' || b==='Inbox')-Number(a==='All' || a==='Inbox'));
    }
    function visualRows() {
        const rows=[];
        for(const group of dropGroups)for(let i=0;i<group.rows.count;i++) {
            const row=group.rows.itemAt(i),p=row.mapToItem(taskScroll.contentItem,0,0);
            rows.push({id:row.modelData.id,x:p.x,y:p.y,height:row.height});
        }
        return rows.sort((a,b)=>Math.abs(a.x-b.x)>1?a.x-b.x:a.y-b.y);
    }
    function focusRow(next) {
        if(!next)return;
        selectedId=next.id;
        const flick=taskScroll.contentItem;
        if(next.y<flick.contentY)flick.contentY=next.y;
        else if(next.y+next.height>flick.contentY+flick.height)flick.contentY=Math.max(0,Math.min(flick.contentHeight-flick.height,next.y+next.height-flick.height));
    }
    function focusVertical(direction) {
        if(collection==='reading'){readingPane.navigate(direction,false);return;}
        const rows=visualRows();
        if(!rows.length)return;
        const index=rows.findIndex(r=>r.id===selectedId);
        focusRow(rows[index<0?(direction>0?0:rows.length-1):(index+direction+rows.length)%rows.length]);
    }
    function focusColumn(direction) {
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
    readonly property bool imageDetails: !editor && collection!=='reading' && !dragTask && !!selected && (selected.images || []).length>0
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
            const inputs = {group:groupInput,tags:tagsInput,due:dueInput,repeat:repeatUnit,reference:planInput,reminder:notifyInput};
            inputs[section].forceActiveFocus();
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
        ToolTip.visible:hovered
        ToolTip.delay:500
        ToolTip.text:description
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
            width:control.width;text:modelData
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
            ScrollView {
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
                            ['⌃ ↵','Save'],['⎋','Cancel']
                        ]:collection==='reading'?[
                            ['N','Save a page'],['D','Mark read / unread'],
                            ['T','Edit page'],['G','Edit category'],
                            ['/','Search pages'],['↵','Open page'],
                            ['← →','Previous / next page'],['↑ ↓ / J K','Select row'],
                            ['P','Remove page'],['U / Ctrl+Z','Undo'],['⎋','Close window']
                        ]:[
                            ['N','New item'],['⇧ N','Today'],
                            ['W','This week'],['L','Later'],
                            ['D','Mark done'],['P','Purge task'],['U / Ctrl+Z','Undo'],
                            ['T','Edit title'],['G','Edit group'],
                            ['S','Snooze'],['/','Search'],
                            ['Ctrl+V','Attach clipboard image'],
                            ['← →','Nearest task in adjacent column'],['↑ ↓ / J K','Select item'],['↵','Open reading reference'],
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
        if(pendingSave || pendingMove)return;
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
    onErrorMessageChanged: {if(errorMessage){if(pendingPurge && (pendingPurge.undoSent || (pendingPurge.sent && !pendingPurge.confirmed)))rejectPurge();if(pendingMove && moveSent)rejectMove();if(dragTask)cancelDrag();if(pendingSave && pendingSave.sent)rejectSave();}}
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
    function action(name, extra) {if(selected && !pendingSave && !pendingPurge && !pendingMove)request(Object.assign({action:name,id:selected.id,period:selected.period,revision:selected.revision},extra || {}));}
    function openEditor(draft) {
        if(pendingSave || pendingPurge || pendingMove)return;
        dueToday=new Date();
        retrySave=null;
        editing=draft || (selected ? JSON.parse(JSON.stringify(selected)) : {title:'',kind:collection,group:'All',tags:[],due:'',repeat:{},reference:''});
        titleInput.text=editing.title;groupInput.editText=editing.group || 'All';
        tagsInput.text=editing.tags.join(', ');dueInput.text=editing.due;
        repeatUnit.currentIndex=Math.max(0,repeatUnit.model.indexOf(editing.repeat.unit || 'none'));
        repeatValue.text=String(editing.repeat.value || 1);planInput.text=editing.reference || editing.plan || '';
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
        if(parsedDue.error){errorMessage=parsedDue.error;dueInput.forceActiveFocus();return;}
        const value={action:draftId?'reading-save':editing.id?'edit':'create',draft_id:draftId,
            id:editing.id || '',revision:editing.revision,
            fields:{kind:editing.kind || collection,title:titleInput.text,group:groupInput.editText.trim() || 'All',
                tags:tagsInput.text.split(',').map(s=>s.trim()).filter(Boolean),due:parsedDue.value,
                repeat:repeatUnit.currentText==='none'?{}:{unit:repeatUnit.currentText,value:repeatValue.text},
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
    onViewChanged: selectVisibleTask()
    onFilterChanged: selectVisibleTask()
    onCollectionChanged: selectVisibleTask()
    function readingSites() {
        return (listState?.tasks || []).filter(t=>t.kind==='reading' && (showCompleted || !t.done) && t.status!=='cancelled' && matchesFilter(t)).sort((a,b)=>readingSort===1?a.title.localeCompare(b.title):readingSort===2?String(a.reference || '').replace(/^https?:\/\/(www\.)?/,'').localeCompare(String(b.reference || '').replace(/^https?:\/\/(www\.)?/,'')):String(b.createdAt || '').localeCompare(String(a.createdAt || '')) || b.id.localeCompare(a.id));
    }
    function visibleCount() {if(collection==='reading')return readingSites().length;return (listState?.groups || []).reduce((n,g)=>n+tasks(g).length,0);}
    function taskMeta(task) {
        const parts=[];
        if(task.saving)parts.push('Saving…');
        if(task.status && !['ready','inbox','done','cancelled','blocked'].includes(task.status))parts.push(task.status.replace('_',' '));
        if(task.assignee)parts.push(task.assignee);
        if(task.verification && task.verification.length)parts.push('Check: '+task.verification[task.verification.length-1].outcome);
        return parts.join(' · ');
    }
    function effectiveDue(task) {
        if(task.snoozedUntil) return Qt.formatDate(new Date(task.snoozedUntil),'yyyy-MM-dd');
        return task.period && task.period!=='once' ? task.period : task.due || '';
    }
    function isOverdue(task) {
        const due=effectiveDue(task);
        return !!due && !task.done && task.status!=='cancelled' && due < (listState?.today || Qt.formatDate(new Date(),'yyyy-MM-dd'));
    }
    function dueLabel(task) {
        const value=effectiveDue(task);
        if(!value)return '·';
        const date=new Date(value+'T12:00:00');
        const today=new Date((listState?.today || Qt.formatDate(new Date(),'yyyy-MM-dd'))+'T12:00:00');
        const delta=Math.round((date-today)/86400000);
        const weekday=(today.getDay()+6)%7;
        if(view!=='all' || (delta>=-weekday && delta<=6-weekday))return ['SUN','MON','TUE','WED','THU','FRI','SAT'][date.getDay()];
        const days=Math.abs(delta);
        if(days<10)return days+'D';
        const start=delta<0?date:today,end=delta<0?today:date;
        const months=(end.getFullYear()-start.getFullYear())*12+end.getMonth()-start.getMonth()-(end.getDate()<start.getDate()?1:0);
        if(months<3)return Math.max(1,Math.floor(days/7))+'W';
        if(months<12)return months+'M';
        return Math.floor(months/12)+'Y';
    }
    function compareTasks(a,b) {
        if(sortOrder===4)return String(a.manualOrder || a.id.replace(/-/g,'')+'8').localeCompare(String(b.manualOrder || b.id.replace(/-/g,'')+'8')) || a.id.localeCompare(b.id);
        if(sortOrder>=2)return (sortOrder===3?-1:1)*a.title.localeCompare(b.title);
        const ad=effectiveDue(a),bd=effectiveDue(b);
        if(!ad || !bd)return ad?-1:bd?1:a.title.localeCompare(b.title);
        return (sortOrder===1?-1:1)*ad.localeCompare(bd) || a.title.localeCompare(b.title);
    }
    function focusList() {
        // Pane is a focus scope: focusing it alone retains the filter's focus.
        filterInput.focus=false;
        app.forceActiveFocus();
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
    function tasks(group) {
        const now=listState?.today || Qt.formatDate(new Date(),'yyyy-MM-dd');
        let end=new Date(now+'T12:00:00');end.setDate(end.getDate()+6-((end.getDay()+6)%7));
        const last=Qt.formatDate(end,'yyyy-MM-dd');
        const rows=(listState?.tasks || []).filter(t=>(t.kind || 'todo')===collection && t.group===group && (showCompleted || !t.done) && (showCancelled || t.status!=='cancelled') && matchesFilter(t) &&
            (view==='all' || (effectiveDue(t) && effectiveDue(t) <= (view==='today'?now:last)) || (t.completedAt && Qt.formatDate(new Date(t.completedAt),'yyyy-MM-dd')===now))).sort(compareTasks);
        if(pendingMove && moveOperation && sortOrder===4 && group===moveOperation.group) {
            const moved=rows.find(t=>t.id===moveOperation.id);
            if(moved) {
                const peers=rows.filter(t=>t.id!==moved.id);
                const before=peers.findIndex(t=>t.id===moveOperation.before_id);
                peers.splice(before<0?peers.length:before,0,moved);
                return peers;
            }
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
        // Keep clicks and wheel events inside the details pane.
        MouseArea {anchors.fill:parent;z:-1;onWheel:wheel=>{wheel.accepted=true;}}
    }
    Keys.onPressed: event=>{
        if(dragTask){if(event.key===Qt.Key_Escape && !pendingMove)cancelDrag();event.accepted=true;return;}
        if(help.visible || dateHelp.visible)return;
        // An open filter must not disable list shortcuts after focus returns to a task.
        if(editor || filterInput.activeFocus){if(event.key===Qt.Key_Escape && editor){cancelEditor();event.accepted=true;}return;}
        if(event.key===Qt.Key_N){if(event.modifiers & Qt.ShiftModifier && collection!=='reading')view='today';else {selectedId='';openEditor();}}
        else if(event.key===Qt.Key_L)view='all';
        else if(event.key===Qt.Key_W && collection!=='reading')view='week';
        else if(event.key===Qt.Key_D)action('toggle');
        else if((event.key===Qt.Key_Return || event.key===Qt.Key_Enter) && collection==='reading')action('open-reference');
        else if(event.key===Qt.Key_T || event.key===Qt.Key_G){if(selected){openEditor();if(event.key===Qt.Key_G)reveal('group');}}
        else if(event.key===Qt.Key_S){if(selected && !pendingSave)snoozeTaskId=snoozeTaskId===selectedId?'':selectedId;}
        else if(event.key===Qt.Key_Slash){searching=true;filterInput.forceActiveFocus();}
        else if(event.key===Qt.Key_Escape){if(snoozeTaskId)snoozeTaskId='';else exitApp();}
        else if(event.key===Qt.Key_Left || event.key===Qt.Key_Right)focusColumn(event.key===Qt.Key_Left?-1:1);
        else if([Qt.Key_Down,Qt.Key_Up,Qt.Key_J,Qt.Key_K,Qt.Key_Tab].includes(event.key)) {
            focusVertical([Qt.Key_Up,Qt.Key_K].includes(event.key)?-1:1);
        }
        event.accepted=true;
    }
    onStateDataChanged: {
        if(pendingMove && moveOperation) {
            const moved=(stateData.tasks || []).find(t=>t.id===moveOperation.id);
            if(moved && moved.revision>moveOperation.revision && moved.group===moveOperation.group && (moved.events || []).slice(-1).some(e=>e.action==='move'))clearMove();
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
                Label {text:collection==='reading'?'Reading':'Tasks';font.pixelSize:24;font.weight:Font.DemiBold;color:theme.text}
                Label {text:editor==='edit'?(draftId?'Save this page to your reading list.':editing && editing.id?'Edit details':'Add a new '+(collection==='reading'?'reading item':'task')):visibleCount()+' '+(collection==='reading'?'saved pages':('tasks · '+(view==='week'?'This week':view==='today'?'Today':'All dates')));font.pixelSize:12;color:theme.muted}
            }
            Item {Layout.fillWidth:true}
            Choice {visible:!editor;model:['Todo','Reading'];currentIndex:collection==='reading'?1:0;enabled:!editor;onActivated:{collection=currentIndex===1?'reading':'todo';view=collection==='reading'?'all':'week';selectVisibleTask();}}
            QuietButton {text:'Open window';visible:canOpenWindow && !editor;enabled:!pendingSave;quiet:true;onClicked:app.openWindow()}
            QuietButton {text:collection==='reading'?'+ Save page':'+ New';visible:!editor;primary:true;enabled:!draftId && !pendingSave;onClicked:{selectedId='';openEditor();}}
        }
        RowLayout {
            visible:!editor;Layout.fillWidth:true;spacing:4
            Repeater {
                model:collection==='reading'?[]:[{label:'Today',value:'today'},{label:'This week',value:'week'},{label:'All tasks',value:'all'}]
                QuietButton {
                    required property var modelData
                    text:modelData.label;quiet:true
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
            Choice {visible:collection!=='reading';model:['Due: soonest','Due: latest','Title: A–Z','Title: Z–A','Manual'];currentIndex:app.sortOrder;implicitWidth:140;onActivated:app.sortOrder=currentIndex}
            IconButton {symbol:'search';description:collection==='reading'?'Filter saved pages (/)':'Filter tasks (/)' ;checkable:true;checked:app.searching;onClicked:{app.searching=checked;if(checked)filterInput.forceActiveFocus();else {filterInput.text='';app.focusList();}}}
            IconButton {symbol:'completed';description:collection==='reading'?'Show read pages':'Show completed tasks';checkable:true;checked:app.showCompleted;onClicked:app.showCompleted=checked}
            IconButton {symbol:collection==='reading'?'categories':'cancelled';description:collection==='reading'?'Group pages by category':'Show cancelled tasks';checkable:true;checked:collection==='reading'?app.readingCategories:app.showCancelled;onClicked:{if(collection==='reading')app.readingCategories=checked;else app.showCancelled=checked}}
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
            visible:!editor && collection==='reading';Layout.fillWidth:true;Layout.fillHeight:true
            theme:app.theme;sites:visible?app.readingSites():[];grid:app.readingGrid;categories:app.readingCategories;selectedId:app.selectedId
            onSelectSite:id=>{app.selectedId=id;app.focusList();}
            onOpenSite:id=>{app.selectedId=id;app.action('open-reference');}
        }
        ScrollView {
            id: taskScroll
            visible:!editor && collection!=='reading';Layout.fillWidth:true;Layout.fillHeight:true;clip:true
            contentWidth:availableWidth
            ScrollBar.horizontal.policy:ScrollBar.AlwaysOff
            ColumnLayout {
                width:taskScroll.availableWidth;spacing:14
                Item {
                    id: groupColumns
                    Layout.fillWidth:true
                    implicitHeight:childrenRect.height
                    readonly property real spacing:32
                    readonly property int columnCount: Math.max(1, Math.floor(taskScroll.availableWidth / 420))
                    readonly property var groups: app.orderedGroups()
                    property var measuredHeights: ({})
                    property var placements: ({})
                    function noteHeight(group,height) {
                        if(app.imageDetails || app.dragTask || app.snoozeTaskId || height<=0 || Math.abs((measuredHeights[group] || 0)-height)<0.5)return;
                        measuredHeights[group]=height;balanceTimer.restart();
                    }
                    function balance() {
                        if(app.imageDetails || app.dragTask || app.snoozeTaskId)return;
                        const loads=Array(columnCount).fill(0),next={};
                        const ordered=groups.map((g,i)=>({group:g,index:i,height:measuredHeights[g] || 30+app.tasks(g).length*48})).filter(item=>app.tasks(item.group).length>0);
                        ordered.sort((a,b)=>Number(b.group==='All' || b.group==='Inbox')-Number(a.group==='All' || a.group==='Inbox') || b.height-a.height || a.index-b.index);
                        for(const item of ordered) {
                            const column=loads.indexOf(Math.min(...loads));
                            next[item.group]=column;loads[column]+=item.height+28;
                        }
                        if(JSON.stringify(next)!==JSON.stringify(placements))placements=next;
                    }
                    onGroupsChanged:balanceTimer.restart()
                    onColumnCountChanged:balanceTimer.restart()
                    Timer {id:balanceTimer;interval:80;onTriggered:groupColumns.balance()}
                    Connections {target:app;function onDragTaskChanged(){if(!app.dragTask)balanceTimer.restart();}}

                    Repeater {
                        model:groupColumns.columnCount
                        Column {
                            id: groupColumn
                            required property int index
                            readonly property int leftCount:Math.ceil(groupColumns.columnCount/2)
                            readonly property bool rightHalf:index>=leftCount
                            readonly property int halfCount:rightHalf?groupColumns.columnCount-leftCount:leftCount
                            width:app.imageDetails?(groupColumns.width/2-16-groupColumns.spacing*(halfCount-1))/halfCount:(groupColumns.width-groupColumns.spacing*(groupColumns.columnCount-1))/groupColumns.columnCount
                            x:app.imageDetails?(rightHalf?groupColumns.width/2+16:0)+(index-(rightHalf?leftCount:0))*(width+groupColumns.spacing):index*(width+groupColumns.spacing)
                            spacing:28
                            Repeater {
                                model:groupColumns.groups.filter((g,i)=>(groupColumns.placements[g] ?? i) % groupColumns.columnCount === groupColumn.index)
                                ColumnLayout {
                                    id:groupPanel
                                    required property string modelData
                                    property alias rows:groupRows
                                    visible:groupRows.count>0 || !!app.dragTask
                                    width:parent.width;spacing:6
                                    onImplicitHeightChanged:groupColumns.noteHeight(modelData,implicitHeight)
                                    Component.onCompleted:{app.dropGroups=app.dropGroups.concat([groupPanel]);groupColumns.noteHeight(modelData,implicitHeight);}
                                    Component.onDestruction:app.dropGroups=app.dropGroups.filter(g=>g!==groupPanel)
                                    RowLayout {
                                        Layout.fillWidth:true;spacing:8
                                        Label {text:modelData==='All'?'Inbox':modelData;font.pixelSize:12;font.weight:Font.DemiBold;color:theme.muted}
                                        Label {text:app.tasks(modelData).length;font.pixelSize:11;color:theme.muted}
                                    }
                                    Repeater {
                                        id:groupRows
                                        model:app.tasks(groupPanel.modelData)
                                        Rectangle {
                                            id:taskCard
                                            required property var modelData
                                            property real gapHeight:app.dragTask && app.dropGroup===groupPanel.modelData && app.dropBefore===modelData.id ? app.dragHeight+6 : 0
                                            Behavior on gapHeight {NumberAnimation {duration:120}}
                                            opacity:app.dragTask && app.dragTask.id===modelData.id ? 0.5 : 1
                                            Layout.fillWidth:true;implicitHeight:cardBody.implicitHeight+8+gapHeight+snoozeReveal.height;radius:4
                                            color:"transparent"
                                            border.color:"transparent"
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
                                            RowLayout {
                                                id:cardBody
                                                z:1
                                                anchors.left:parent.left;anchors.right:parent.right;anchors.top:parent.top;anchors.margins:4;anchors.topMargin:4+taskCard.gapHeight;spacing:8
                                                Item {
                                                    readonly property bool focusedTask: selectedId===modelData.id
                                                    Layout.preferredWidth:30;Layout.preferredHeight:taskTitle.firstLineHeight;Layout.alignment:Qt.AlignTop
                                                    Canvas {
                                                        visible:parent.focusedTask
                                                        width:12;height:12;anchors.verticalCenter:parent.verticalCenter
                                                        property color ink:theme.accent
                                                        onInkChanged:requestPaint()
                                                        onPaint:{
                                                            const c=getContext('2d');c.reset();
                                                            c.strokeStyle=ink;c.lineWidth=1.8;c.lineCap='round';c.lineJoin='round';
                                                            c.beginPath();c.moveTo(4,3);c.lineTo(7,6);c.lineTo(4,9);c.stroke();
                                                        }
                                                    }
                                                    Label {
                                                        visible:!parent.focusedTask
                                                        y:3;text:app.dueLabel(modelData)
                                                        color:app.isOverdue(modelData)?'#eb6f92':theme.muted
                                                        font.family:'monospace';font.pixelSize:11;font.weight:Font.DemiBold
                                                        font.underline:!!modelData.snoozedUntil
                                                    }
                                                }
                                                ColumnLayout {
                                                    Layout.fillWidth:true;spacing:3
                                                    Label {
                                                        id: taskTitle
                                                        Layout.fillWidth:true
                                                        Layout.rightMargin:statusIcons.visible?statusIcons.implicitWidth+6:0
                                                        property real lastLineWidth: 0
                                                        property real lastLineTop: 0
                                                        property real firstLineHeight: 18
                                                        text:modelData.title
                                                        textFormat:Text.PlainText
                                                        font.pixelSize:15
                                                        font.weight:selectedId===modelData.id?Font.DemiBold:Font.Normal
                                                        font.strikeout:modelData.done && modelData.status!=='cancelled'
                                                        color:selectedId===modelData.id?theme.accent:theme.text
                                                        wrapMode:Text.Wrap;maximumLineCount:3;elide:Text.ElideRight
                                                        property var strikeLines: []
                                                        onLineLaidOut: line => {
                                                            if(line.number===0) {strikeLines=[];firstLineHeight=line.height;}
                                                            strikeLines.push({x:line.x,y:line.y+line.height*0.52,width:Math.min(line.width,line.implicitWidth)});
                                                            lastLineWidth=Math.min(line.width,line.implicitWidth);
                                                            lastLineTop=line.y;
                                                            cancelledStrike.requestPaint();
                                                        }
                                                        onColorChanged: cancelledStrike.requestPaint()
                                                        Row {
                                                            id:statusIcons
                                                            x:taskTitle.lastLineWidth+6;y:taskTitle.lastLineTop+2
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
                                                            onVisibleChanged:requestPaint()
                                                            onWidthChanged:requestPaint()
                                                            onHeightChanged:requestPaint()
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
                                                onClicked:{if(!moved){selectedId=modelData.id;app.focusList();}}
                                                onDoubleClicked:{if(!moved)app.action('toggle');}
                                            }
                                        }
                                    }
                                    Item {
                                        Layout.fillWidth:true
                                        implicitHeight:app.dragTask && app.dropGroup===groupPanel.modelData && app.dropBefore===''?app.dragHeight+6:groupRows.count===0?30:0
                                        Behavior on implicitHeight {NumberAnimation {duration:120}}
                                        Label {visible:groupRows.count===0;anchors.centerIn:parent;text:'Drop tasks here';color:theme.muted;font.pixelSize:11}
                                    }
                                }
                            }
                        }
                    }
                }
                ColumnLayout {
                    visible:visibleCount()===0;Layout.fillWidth:true;Layout.topMargin:32;spacing:10
                    Label {text:searching && filter?'No matching tasks':'Nothing scheduled here';color:theme.text;font.pixelSize:18;Layout.alignment:Qt.AlignHCenter}
                    Label {text:view==='all'?'Add a task to get started.':'Check All tasks for upcoming or undated items.';color:theme.muted;font.pixelSize:13;wrapMode:Text.Wrap;Layout.fillWidth:true;horizontalAlignment:Text.AlignHCenter}
                }
            }
        }
        RowLayout {
            visible:!editor;Layout.fillWidth:true;spacing:4
            QuietButton {text:collection==='reading'?(selected && selected.done?'Mark unread':'Mark read'):(selected && selected.done?'Mark undone':'Mark done');enabled:!!selected;quiet:true;onClicked:action('toggle')}
            QuietButton {text:'Edit';enabled:!!selected;quiet:true;onClicked:openEditor()}
            Item {Layout.fillWidth:true}
            IconButton {symbol:'undo';description:'Undo (U / Ctrl+Z)';onClicked:app.undoLastAction()}
            IconButton {visible:collection!=='reading';symbol:'info';description:'Due dates and snoozing';onClicked:dateHelp.open()}
            IconButton {symbol:'keyboard';description:'Keyboard shortcuts (?)';onClicked:help.open()}
        }
        ScrollView {
            id:editorScroll;visible:editor==='edit';enabled:visible;Layout.fillWidth:true;Layout.fillHeight:true;clip:true;contentWidth:availableWidth;contentHeight:editorForm.implicitHeight;ScrollBar.horizontal.policy:ScrollBar.AlwaysOff
            ColumnLayout {
                id:editorForm
                width:editorScroll.availableWidth;height:implicitHeight;spacing:18
                ColumnLayout {
                    Layout.fillWidth:true;spacing:6
                    Label {text:'Title';color:theme.muted;font.pixelSize:12}
                    Entry {id:titleInput;Layout.fillWidth:true;placeholderText:collection==='reading'?'Page title':'What needs to be done?';font.pixelSize:18;onAccepted:if(text.trim())saveEditor()}
                }
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
                Flow {
                    id:detailButtons
                    Layout.fillWidth:true;Layout.preferredHeight:implicitHeight;Layout.maximumHeight:implicitHeight;spacing:6
                    Repeater {
                        model:[{label:'Domain',key:'group'},{label:'Tags',key:'tags'},{label:'Due date',key:'due'},{label:'Repeat',key:'repeat'},{label:'Note',key:'reference'},{label:'Reminder',key:'reminder'}]
                        QuietButton {required property var modelData;objectName:'add-'+modelData.key;text:'+ '+modelData.label;visible:!sections[modelData.key];implicitHeight:34;horizontalPadding:12;onClicked:reveal(modelData.key)}
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
                        Choice {id:repeatUnit;model:['none','days','weeks','months','years','days-of-the-week','days-of-the-month']}
                        Entry {id:repeatValue;Layout.fillWidth:true;placeholderText:'Interval or day numbers'}
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
                    QuietButton {text:'Cancel';enabled:!pendingSave;onClicked:cancelEditor()}
                }
            }
        }
        Label {visible:!!pendingSave;text:'Saving “'+(pendingSave?pendingSave.draft.title:'')+'”…';color:theme.accent;wrapMode:Text.Wrap;Layout.fillWidth:true}
        Label {visible:!!stateData.sync_message;text:stateData.sync_message || '';color:theme.accent;wrapMode:Text.Wrap;Layout.fillWidth:true}
        Label {visible:!!errorMessage || !!moveMessage || !!stateData.message;text:errorMessage || moveMessage || stateData.message || '';color:theme.accent;wrapMode:Text.Wrap;Layout.fillWidth:true}
    }
    function accepted() {
        if(pendingSave) {
            if(!pendingSave.sent)return;
            const value=pendingSave.request;
            const saved=(stateData.tasks || []).find(t=>value.id?t.id===value.id:t.createKey===value.request_id);
            if(saved)selectedId=saved.id;
            pendingSave=null;retrySave=null;
        }
        draftId='';editor='';app.focusList();
    }
}
