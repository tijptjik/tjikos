import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

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
    property int sortOrder: 0
    property bool requestBusy: false
    property var dragTask: null
    property var dragSnapshot: ({})
    readonly property var listState: dragTask ? dragSnapshot : stateData
    property var dragSource: null
    property point dragPoint: Qt.point(0,0)
    property real dragWidth: 300
    property real dragHeight: 40
    property var dropGroups: []
    property string dropGroup: ''
    property string dropBefore: ''
    property bool pendingMove: false
    property bool moveSent: false
    property string moveMessage: ''
    onSortOrderChanged: {if(dragTask && !pendingMove)cancelDrag();}
    function cancelDrag() {
        moveTimeout.stop();pendingMove=false;moveSent=false;
        dropGroup='';dropBefore='';dragTask=null;dragSource=null;
    }
    function startDrag(task, source, width, height, point) {
        if(sortOrder!==4 || pendingMove)return;
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
        pendingMove=true;moveTimeout.restart();sendMove();
    }
    function sendMove() {
        if(!pendingMove || moveSent || requestBusy)return;
        moveSent=true;
        request({action:'move',id:dragTask.id,revision:dragTask.revision,group:dropGroup,before_id:dropBefore});
    }
    Timer {interval:50;repeat:true;running:app.pendingMove && !app.moveSent;onTriggered:app.sendMove()}
    Timer {id:moveTimeout;interval:15000;onTriggered:{app.cancelDrag();app.moveMessage='Move was not confirmed; refresh and try again.';}}
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
    function focusColumn(direction) {
        const rows=[];
        for(const group of dropGroups)for(let i=0;i<group.rows.count;i++) {
            const row=group.rows.itemAt(i),p=row.mapToItem(taskScroll.contentItem,0,0);
            rows.push({id:row.modelData.id,x:p.x,y:p.y,height:row.height});
        }
        const current=rows.find(r=>r.id===selectedId);
        if(!current)return;
        const candidates=rows.filter(r=>(r.x-current.x)*direction>1);
        if(!candidates.length)return;
        const distance=Math.min(...candidates.map(r=>Math.abs(r.x-current.x)));
        const adjacent=candidates.filter(r=>Math.abs(Math.abs(r.x-current.x)-distance)<1);
        adjacent.sort((a,b)=>Math.abs(a.y-current.y)-Math.abs(b.y-current.y));
        const next=adjacent[0];selectedId=next.id;
        const flick=taskScroll.contentItem;
        if(next.y<flick.contentY)flick.contentY=next.y;
        else if(next.y+next.height>flick.contentY+flick.height)flick.contentY=Math.max(0,Math.min(flick.contentHeight-flick.height,next.y+next.height-flick.height));
    }
    property var selected: (stateData.tasks || []).find(t=>t.id===selectedId) || null
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
            Connections {target:control;function onCheckedChanged(){iconCanvas.requestPaint();}}
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
    Shortcut {sequence:'P';autoRepeat:false;enabled:!editor && !searching && !help.visible && !dateHelp.visible && !!selected && !pendingPurge;onActivated:app.purgeSelected()}
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
                        ]:[
                            ['N','New item'],['⇧ N','Today'],
                            ['W','This week'],['L','Later'],
                            ['D','Mark done'],['P','Purge task'],['U / Ctrl+Z','Undo'],
                            ['T','Edit title'],['G','Edit group'],
                            ['S','Snooze'],['/','Search'],
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
    property string undoToastTask: ''
    property string undoToastTitle: ''
    function purgeSelected() {
        if(!selected || pendingPurge)return;
        pendingPurge={id:selected.id,title:selected.title};
        purgePendingTimer.restart();
        request({action:'purge',id:selected.id,revision:selected.revision});
    }
    function undoLastAction() {
        const task=undoToastTask;
        undoToastTask='';toastTimer.stop();
        request(task?{action:'undo',id:task,undo_action:'purge'}:{action:'undo'});
    }
    onErrorMessageChanged: {if(errorMessage){pendingPurge=null;purgePendingTimer.stop();if(dragTask)cancelDrag();}}
    Timer {id:purgePendingTimer;interval:15000;onTriggered:app.pendingPurge=null;}
    Timer {id:toastTimer;interval:3000;onTriggered:app.undoToastTask='';}
    Rectangle {
        id:purgeToast
        z:100
        anchors.horizontalCenter:parent.horizontalCenter
        anchors.bottom:parent.bottom;anchors.bottomMargin:8
        width:Math.min(app.width-16,440);height:54;radius:10
        color:theme.surface
        visible:app.undoToastTask!==''
        RowLayout {
            anchors.fill:parent;anchors.margins:8;spacing:12
            Label {Layout.fillWidth:true;text:'Deleted “'+app.undoToastTitle+'”';elide:Text.ElideRight;color:theme.text}
            QuietButton {text:'Undo';quiet:true;onClicked:app.undoLastAction()}
        }
    }
    signal request(var request)
    signal exitApp()
    signal openWindow()
    property bool canOpenWindow: false
    function action(name, extra) {if(selected)request(Object.assign({action:name,id:selected.id,period:selected.period,revision:selected.revision},extra || {}));}
    function openEditor(draft) {
        editing=draft || (selected ? JSON.parse(JSON.stringify(selected)) : {title:'',kind:collection,group:'All',tags:[],due:'',repeat:{},reference:''});
        titleInput.text=editing.title;groupInput.editText=editing.group || 'All';
        tagsInput.text=editing.tags.join(', ');dueInput.text=editing.due;
        repeatUnit.currentIndex=Math.max(0,repeatUnit.model.indexOf(editing.repeat.unit || 'none'));
        repeatValue.text=String(editing.repeat.value || 1);planInput.text=editing.reference || editing.plan || '';
        notifyInput.checked=editing.notifyBeforeDue!==false;
        leadValue.text=String(editing.notifyBeforeDueLeadValue ?? 1);
        leadUnit.currentIndex=Math.max(0,leadUnit.model.indexOf(editing.notifyBeforeDueLeadUnit || 'days'));
        sections = {group:!!editing.id && editing.group!=='All',tags:editing.tags.length>0,
            due:!!editing.due,repeat:!!editing.repeat.unit,reference:!!planInput.text,
            reminder:!!editing.id && (editing.notifyBeforeDue===false || Number(leadValue.text)!==1 || leadUnit.currentText!=='days')};
        planPicker.visible=false;
        editor='edit';titleInput.forceActiveFocus();
    }
    function cancelEditor() {
        if(draftId)request({action:'reading-cancel',draft_id:draftId});
        else {editor='';app.forceActiveFocus();}
    }
    function saveEditor() {
        request({action:draftId?'reading-save':editing.id?'edit':'create',draft_id:draftId,
            id:editing.id || '',revision:editing.revision,
            fields:{kind:editing.kind || collection,title:titleInput.text,group:groupInput.editText.trim() || 'All',
                tags:tagsInput.text.split(',').map(s=>s.trim()).filter(Boolean),due:dueInput.text,
                repeat:repeatUnit.currentText==='none'?{}:{unit:repeatUnit.currentText,value:repeatValue.text},
                reference:planInput.text,notifyBeforeDue:notifyInput.checked,
                notifyBeforeDueLeadValue:Number(leadValue.text),notifyBeforeDueLeadUnit:leadUnit.currentText}});
    }
    function selectVisibleTask() {
        const rows=(listState?.groups || []).reduce((items,g)=>items.concat(tasks(g)),[]);
        if(!rows.some(t=>t.id===selectedId))selectedId=rows.length?rows[0].id:'';
    }
    onViewChanged: selectVisibleTask()
    onFilterChanged: selectVisibleTask()
    onCollectionChanged: selectVisibleTask()
    function visibleCount() {return (listState?.groups || []).reduce((n,g)=>n+tasks(g).length,0);}
    function taskMeta(task) {
        const parts=[];
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
        return (listState?.tasks || []).filter(t=>(t.kind || 'todo')===collection && t.group===group && (showCompleted || !t.done) && (showCancelled || t.status!=='cancelled') && matchesFilter(t) &&
            (view==='all' || (effectiveDue(t) && effectiveDue(t) <= (view==='today'?now:last)) || (t.completedAt && Qt.formatDate(new Date(t.completedAt),'yyyy-MM-dd')===now))).sort(compareTasks);
    }
    Keys.onPressed: event=>{
        if(dragTask){if(event.key===Qt.Key_Escape && !pendingMove)cancelDrag();event.accepted=true;return;}
        if(help.visible || dateHelp.visible)return;
        if(editor || searching){if(event.key===Qt.Key_Escape && editor){cancelEditor();event.accepted=true;}return;}
        if(event.key===Qt.Key_N){if(event.modifiers & Qt.ShiftModifier)view='today';else {selectedId='';openEditor();}}
        else if(event.key===Qt.Key_L)view='all';
        else if(event.key===Qt.Key_W)view='week';
        else if(event.key===Qt.Key_D)action('toggle');
        else if((event.key===Qt.Key_Return || event.key===Qt.Key_Enter) && collection==='reading')action('open-reference');
        else if(event.key===Qt.Key_T || event.key===Qt.Key_G){if(selected){openEditor();if(event.key===Qt.Key_G)reveal('group');}}
        else if(event.key===Qt.Key_S){if(selected)editor='snooze';}
        else if(event.key===Qt.Key_Slash){searching=true;filterInput.forceActiveFocus();}
        else if(event.key===Qt.Key_Escape){if(snoozeTaskId)snoozeTaskId='';else exitApp();}
        else if(event.key===Qt.Key_Left || event.key===Qt.Key_Right)focusColumn(event.key===Qt.Key_Left?-1:1);
        else if([Qt.Key_Down,Qt.Key_Up,Qt.Key_J,Qt.Key_K,Qt.Key_Tab].includes(event.key)) {
            const rows=(stateData.groups || []).reduce((rows,g)=>rows.concat(tasks(g)),[]);
            if(rows.length){let i=rows.findIndex(t=>t.id===selectedId);i=(i+([Qt.Key_Up,Qt.Key_K].includes(event.key)?-1:1)+rows.length)%rows.length;selectedId=rows[i].id;}
        }
        event.accepted=true;
    }
    onStateDataChanged: {
        if(pendingMove && dragTask) {
            const moved=(stateData.tasks || []).find(t=>t.id===dragTask.id);
            if(moved && moved.revision>dragTask.revision && moved.group===dropGroup && (moved.events || []).slice(-1).some(e=>e.action==='move'))cancelDrag();
        }
        if(pendingPurge && Array.isArray(stateData.tasks) && !stateData.tasks.some(t=>t.id===pendingPurge.id)) {
            undoToastTask=pendingPurge.id;undoToastTitle=pendingPurge.title;pendingPurge=null;
            purgePendingTimer.stop();toastTimer.restart();
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
                Label {text:editor==='edit'?'Keep the next step clear.':visibleCount()+' '+(collection==='reading'?'items':'tasks')+' · '+(view==='week'?'This week':view==='today'?'Today':'All dates');font.pixelSize:12;color:theme.muted}
            }
            Item {Layout.fillWidth:true}
            Choice {model:['Todo','Reading'];currentIndex:collection==='reading'?1:0;enabled:!editor;onActivated:{collection=currentIndex===1?'reading':'todo';view=collection==='reading'?'all':'week';selectVisibleTask();}}
            QuietButton {text:'Open window';visible:canOpenWindow;enabled:!editor;quiet:true;onClicked:app.openWindow()}
            QuietButton {text:'+ New';primary:true;enabled:!draftId && !editor;onClicked:{selectedId='';openEditor();}}
        }
        RowLayout {
            visible:!editor;Layout.fillWidth:true;spacing:4
            Repeater {
                model:[{label:'Today',value:'today'},{label:'This week',value:'week'},{label:'All tasks',value:'all'}]
                QuietButton {
                    required property var modelData
                    text:modelData.label;quiet:true
                    checked:app.view===modelData.value
                    background:Rectangle {radius:9;color:parent.checked?theme.surface:'transparent';border.color:parent.checked?theme.border:'transparent'}
                    onClicked:{app.view=modelData.value;app.forceActiveFocus();}
                }
            }
            Item {Layout.fillWidth:true}
            Choice {model:['Due: soonest','Due: latest','Title: A–Z','Title: Z–A','Manual · drag'];currentIndex:app.sortOrder;implicitWidth:140;onActivated:app.sortOrder=currentIndex}
            IconButton {symbol:'search';description:'Filter tasks (/)';checkable:true;checked:app.searching;onClicked:{app.searching=checked;if(checked)filterInput.forceActiveFocus();else {filterInput.text='';app.forceActiveFocus();}}}
            IconButton {symbol:'completed';description:'Show completed tasks';checkable:true;checked:app.showCompleted;onClicked:app.showCompleted=checked}
            IconButton {symbol:'cancelled';description:'Show cancelled tasks';checkable:true;checked:app.showCancelled;onClicked:app.showCancelled=checked}
            IconButton {symbol:'details';description:'Show selected task details';checkable:true;checked:app.showDetails;onClicked:app.showDetails=checked}
        }
        Entry {color:theme.text;placeholderTextColor:theme.muted;id:filterInput;visible:!editor && searching;Layout.fillWidth:true;placeholderText:collection==='reading'?'Filter reading list':'Filter tasks';onTextChanged:app.filter=text;Keys.onEscapePressed:{searching=false;filter='';text='';app.forceActiveFocus();}}
        Label {
            visible:!editor && showDetails && !!selected && text!==''
            Layout.fillWidth:true;wrapMode:Text.Wrap;color:theme.muted
            text:selected ? (selected.blockers || []).map(b=>'Blocked: '+b.reason+(b.task_id?' ['+b.task_id+']':'')).concat(
                (selected.acceptance || []).map(c=>'Accept: '+c),
                (selected.verification || []).slice(-1).map(v=>v.outcome+' by '+v.actor+' · '+v.artifact+' · '+v.evidence)).join('\n') : ''
        }
        ScrollView {
            id: taskScroll
            visible:!editor;Layout.fillWidth:true;Layout.fillHeight:true;clip:true
            contentWidth:availableWidth
            ScrollBar.horizontal.policy:ScrollBar.AlwaysOff
            ColumnLayout {
                width:taskScroll.availableWidth;spacing:14
                Row {
                    id: groupColumns
                    Layout.fillWidth:true
                    spacing:32
                    readonly property int columnCount: Math.max(1, Math.floor(taskScroll.availableWidth / 420))
                    readonly property var groups: app.orderedGroups().filter(g=>app.tasks(g).length>0 || app.sortOrder===4)
                    property var measuredHeights: ({})
                    property var placements: ({})
                    function noteHeight(group,height) {
                        if(app.dragTask || app.snoozeTaskId || height<=0 || Math.abs((measuredHeights[group] || 0)-height)<0.5)return;
                        measuredHeights[group]=height;balanceTimer.restart();
                    }
                    function balance() {
                        if(app.dragTask || app.snoozeTaskId)return;
                        const loads=Array(columnCount).fill(0),next={};
                        const ordered=groups.map((g,i)=>({group:g,index:i,height:measuredHeights[g] || 30+app.tasks(g).length*48}));
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
                            width:(groupColumns.width - groupColumns.spacing * (groupColumns.columnCount - 1)) / groupColumns.columnCount
                            spacing:28
                            Repeater {
                                model:groupColumns.groups.filter((g,i)=>(groupColumns.placements[g] ?? i) % groupColumns.columnCount === groupColumn.index)
                                ColumnLayout {
                                    id:groupPanel
                                    required property string modelData
                                    property alias rows:groupRows
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
                                                        onChosen:until=>{app.action('snooze',{until:until});app.snoozeTaskId='';app.forceActiveFocus();}
                                                    }
                                                    QuietButton {text:'Cancel';quiet:true;onClicked:{app.snoozeTaskId='';app.forceActiveFocus();}}
                                                }
                                            }
                                            RowLayout {
                                                id:cardBody
                                                z:1
                                                anchors.left:parent.left;anchors.right:parent.right;anchors.top:parent.top;anchors.margins:4;anchors.topMargin:4+taskCard.gapHeight;spacing:8
                                                Label {
                                                    readonly property bool focusedTask: selectedId===modelData.id
                                                    text:focusedTask?'›':app.dueLabel(modelData)
                                                    color:focusedTask?theme.accent:app.isOverdue(modelData)?'#eb6f92':theme.muted
                                                    font.family:'monospace';font.pixelSize:focusedTask?20:11;font.weight:Font.DemiBold
                                                    font.underline:!focusedTask && !!modelData.snoozedUntil
                                                    Layout.preferredWidth:30;Layout.topMargin:focusedTask?0:3;Layout.alignment:Qt.AlignTop
                                                }
                                                ColumnLayout {
                                                    Layout.fillWidth:true;spacing:3
                                                    Label {
                                                        id: taskTitle
                                                        Layout.fillWidth:true
                                                        Layout.rightMargin:statusIcons.visible?statusIcons.implicitWidth+6:0
                                                        property real lastLineWidth: 0
                                                        property real lastLineTop: 0
                                                        text:modelData.title
                                                        textFormat:Text.PlainText
                                                        font.pixelSize:15
                                                        font.weight:selectedId===modelData.id?Font.DemiBold:Font.Normal
                                                        font.strikeout:modelData.done && modelData.status!=='cancelled'
                                                        color:selectedId===modelData.id?theme.accent:theme.text
                                                        wrapMode:Text.Wrap;maximumLineCount:3;elide:Text.ElideRight
                                                        property var strikeLines: []
                                                        onLineLaidOut: line => {
                                                            if(line.number===0) strikeLines=[];
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
                                                            visible:blockedIcon.visible || repeatIcon.visible
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
                                                cursorShape:app.sortOrder===4?Qt.OpenHandCursor:Qt.PointingHandCursor
                                                preventStealing:app.sortOrder===4
                                                property point pressedPoint
                                                property bool moved: false
                                                onPressed: mouse=>{pressedPoint=mapToItem(app,mouse.x,mouse.y);moved=false;selectedId=modelData.id;app.forceActiveFocus();}
                                                onPositionChanged: mouse=>{
                                                    if(!pressed || app.sortOrder!==4 || app.pendingMove)return;
                                                    const point=mapToItem(app,mouse.x,mouse.y);
                                                    if(!moved && Math.hypot(point.x-pressedPoint.x,point.y-pressedPoint.y)>10){moved=true;app.startDrag(modelData,cardBody,taskCard.width,cardBody.implicitHeight+8,point);}
                                                    if(moved && app.dragTask)app.updateDrag(point);
                                                }
                                                onReleased: {if(moved)app.finishDrag();}
                                                onCanceled: {if(moved && !app.pendingMove)app.cancelDrag();}
                                                onClicked:{if(!moved){selectedId=modelData.id;app.forceActiveFocus();}}
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
            QuietButton {text:selected && selected.done?'Mark undone':'Mark done';enabled:!!selected;quiet:true;onClicked:action('toggle')}
            QuietButton {text:'Edit';enabled:!!selected;quiet:true;onClicked:openEditor()}
            Item {Layout.fillWidth:true}
            IconButton {symbol:'undo';description:'Undo (U / Ctrl+Z)';onClicked:app.undoLastAction()}
            IconButton {symbol:'info';description:'Due dates and snoozing';onClicked:dateHelp.open()}
            IconButton {symbol:'keyboard';description:'Keyboard shortcuts (?)';onClicked:help.open()}
        }
        ScrollView {
            id:editorScroll;visible:editor==='edit';Layout.fillWidth:true;Layout.fillHeight:true;clip:true;contentWidth:availableWidth;ScrollBar.horizontal.policy:ScrollBar.AlwaysOff
            ColumnLayout {
                width:editorScroll.availableWidth;spacing:12
                Label {text:draftId?'Read later · save before closing tab':editing && editing.id?'Edit item':'New item';color:theme.text}
                Entry {color:theme.text;placeholderTextColor:theme.muted;id:titleInput;Layout.fillWidth:true;placeholderText:'What needs to be done?';font.pixelSize:20;onAccepted:if(text.trim())saveEditor()}
                Flow {
                    Layout.fillWidth:true;Layout.preferredHeight:childrenRect.height;spacing:6
                    Repeater {
                        model:[{label:'Domain',key:'group'},{label:'Tags',key:'tags'},{label:'Due date',key:'due'},{label:'Repeat',key:'repeat'},{label:'Note',key:'reference'},{label:'Reminder',key:'reminder'}]
                        QuietButton {required property var modelData;text:'+ '+modelData.label;visible:!sections[modelData.key];onClicked:reveal(modelData.key)}
                    }
                }
                Label {visible:!!sections.group;text:'Domain';color:theme.muted}
                Choice {id:groupInput;visible:!!sections.group;Layout.fillWidth:true;editable:true;model:stateData.groups || [];inputMethodHints:Qt.ImhNoPredictiveText}
                Entry {color:theme.text;placeholderTextColor:theme.muted;id:tagsInput;visible:!!sections.tags;Layout.fillWidth:true;placeholderText:'Tags, comma separated'}
                Entry {color:theme.text;placeholderTextColor:theme.muted;id:dueInput;visible:!!sections.due;Layout.fillWidth:true;placeholderText:'Due date · YYYY-MM-DD'}
                Label {visible:!!sections.repeat;text:'Repeat';color:theme.muted}
                RowLayout {
                    visible:!!sections.repeat
                    Choice {id:repeatUnit;model:['none','days','weeks','months','years','days-of-the-week','days-of-the-month']}
                    Entry {color:theme.text;placeholderTextColor:theme.muted;id:repeatValue;Layout.fillWidth:true;placeholderText:'Interval or comma-separated day numbers'}
                }
                Label {visible:!!sections.reference;text:'Reference';color:theme.muted}
                RowLayout {
                    visible:!!sections.reference
                    Entry {color:theme.text;placeholderTextColor:theme.muted;id:planInput;Layout.fillWidth:true;placeholderText:editing && editing.kind==='reading'?'Reference · https://…':'Reference · Obsidian vault path'}
                    QuietButton {text:'Find note';visible:!editing || editing.kind!=='reading';onClicked:{request({action:'plans'});planPicker.visible=true;}}
                }
                ColumnLayout {
                    id:planPicker;visible:false;Layout.fillWidth:true
                    Entry {color:theme.text;placeholderTextColor:theme.muted;id:planFilter;placeholderText:'Fuzzy file search';Layout.fillWidth:true}
                    Repeater {
                        model:plans.filter(p=>{let i=0;for(const c of p.toLowerCase())if(c===planFilter.text.toLowerCase()[i])i++;return i===planFilter.text.length;}).slice(0,15)
                        QuietButton {required property string modelData;text:modelData;Layout.fillWidth:true;onClicked:{planInput.text=modelData;planPicker.visible=false;}}
                    }
                }
                RowLayout {
                    visible:!!sections.reminder
                    CheckBox {id:notifyInput;text:'Remind before due';checked:true}
                    Entry {id:leadValue;Layout.preferredWidth:60;text:'1';validator:IntValidator {bottom:0;top:10000}}
                    Choice {id:leadUnit;model:['hours','days','weeks'];currentIndex:1}
                }
                RowLayout {
                    Layout.topMargin:8;spacing:8
                    QuietButton {primary:true;text:draftId?'Save and close tab':'Save';enabled:titleInput.text.trim()!=='';onClicked:saveEditor()}
                    QuietButton {text:'Cancel';onClicked:cancelEditor()}
                }
            }
        }
        Label {visible:!!stateData.sync_message;text:stateData.sync_message || '';color:theme.accent;wrapMode:Text.Wrap;Layout.fillWidth:true}
        Label {visible:!!errorMessage || !!moveMessage || !!stateData.message;text:errorMessage || moveMessage || stateData.message || '';color:theme.accent;wrapMode:Text.Wrap;Layout.fillWidth:true}
    }
    function accepted() {draftId='';editor='';app.forceActiveFocus();}
}
