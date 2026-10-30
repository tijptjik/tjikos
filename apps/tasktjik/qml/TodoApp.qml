import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Pane {
    padding: 0
    background: Item {}
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
    property string filter: ''
    property string selectedId: ''
    property var editing: null
    property string editor: ''
    property var selected: (stateData.tasks || []).find(t=>t.id===selectedId) || null
    property string errorMessage: ""
    property var plans: []
    signal request(var request)
    signal exitApp()
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
    function tasks(group) {
        const now=stateData.today || Qt.formatDate(new Date(),'yyyy-MM-dd');
        let end=new Date(now+'T12:00:00');end.setDate(end.getDate()+6-((end.getDay()+6)%7));
        const last=Qt.formatDate(end,'yyyy-MM-dd');
        return (stateData.tasks || []).filter(t=>(t.kind || 'todo')===collection && t.group===group && (!filter || (t.title+' '+t.tags.join(' ')+' '+t.group+' '+(t.reference || '')).toLowerCase().includes(filter.toLowerCase())) &&
            (view==='all' || (t.period!=='once' && t.period <= (view==='today'?now:last)) || (t.completedAt && Qt.formatDate(new Date(t.completedAt),'yyyy-MM-dd')===now)));
    }
    Keys.onPressed: event=>{
        if(editor || searching){if(event.key===Qt.Key_Escape && editor){cancelEditor();event.accepted=true;}return;}
        if(event.key===Qt.Key_N){if(event.modifiers & Qt.ShiftModifier)view='today';else {selectedId='';openEditor();}}
        else if(event.key===Qt.Key_L)view='all';
        else if(event.key===Qt.Key_W)view='week';
        else if(event.key===Qt.Key_D)action('toggle');
        else if((event.key===Qt.Key_Return || event.key===Qt.Key_Enter) && collection==='reading')action('open-reference');
        else if(event.key===Qt.Key_T || event.key===Qt.Key_G){if(selected){openEditor();if(event.key===Qt.Key_G)groupInput.forceActiveFocus();}}
        else if(event.key===Qt.Key_S){if(selected)editor='snooze';}
        else if(event.key===Qt.Key_Slash){searching=true;filterInput.forceActiveFocus();}
        else if(event.key===Qt.Key_Escape)exitApp();
        else if([Qt.Key_Down,Qt.Key_Up,Qt.Key_J,Qt.Key_K,Qt.Key_Tab].includes(event.key)) {
            const rows=(stateData.groups || []).reduce((rows,g)=>rows.concat(tasks(g)),[]);
            if(rows.length){let i=rows.findIndex(t=>t.id===selectedId);i=(i+([Qt.Key_Up,Qt.Key_K].includes(event.key)?-1:1)+rows.length)%rows.length;selectedId=rows[i].id;}
        }
        event.accepted=true;
    }
    onStateDataChanged: {
        if(stateData.plans)plans=stateData.plans;
        if(stateData.collection && collection!==stateData.collection){collection=stateData.collection;view=collection==='reading'?'all':'week';}
        if(stateData.draft_id && stateData.draft_id!==lastDraftId){
            draftId=stateData.draft_id;lastDraftId=draftId;
            Qt.callLater(()=>openEditor(stateData.draft));
        }
        if(!selected && stateData.tasks){const rows=(stateData.groups || []).reduce((rows,g)=>rows.concat(tasks(g)),[]);selectedId=rows.length?rows[0].id:'';}
    }
    ColumnLayout {
        anchors.fill:parent;spacing:10
        RowLayout {
            Label {text:(collection==='reading'?'READING · ':'TODO · ')+(app.view==='week'?'This week':app.view==='today'?'Today':'Later');font.pixelSize:22;color:theme.text;Layout.fillWidth:true}
            ComboBox {model:['Todo','Reading'];currentIndex:collection==='reading'?1:0;enabled:!editor;onActivated:{collection=currentIndex===1?'reading':'todo';view=collection==='reading'?'all':'week';selectedId='';}}
            Button {text:'New';enabled:!draftId;onClicked:{selectedId='';openEditor();}}
            Button {text:'×';onClicked:{if(draftId)cancelEditor();else app.exitApp();}}
        }
        TextField {color:theme.text;placeholderTextColor:theme.muted;id:filterInput;visible:searching;Layout.fillWidth:true;placeholderText:collection==='reading'?'Filter reading list':'Filter tasks';onTextChanged:app.filter=text;Keys.onEscapePressed:{searching=false;filter='';text='';app.forceActiveFocus();}}
        Label {visible:!editor;text:(collection==='reading'?'Enter open · ':'')+'n new · Shift+N now · w week · l later · d done · t title · g group · s snooze · / search';color:theme.muted;font.pixelSize:11;wrapMode:Text.Wrap;Layout.fillWidth:true}
        Label {
            visible:!!selected
            Layout.fillWidth:true;wrapMode:Text.Wrap;color:theme.muted
            text:selected ? (selected.blockers || []).map(b=>'Blocked: '+b.reason+(b.task_id?' ['+b.task_id+']':'')).concat(
                (selected.acceptance || []).map(c=>'Accept: '+c),
                (selected.verification || []).slice(-1).map(v=>v.outcome+' by '+v.actor+' · '+v.artifact+' · '+v.evidence)).join('\n') : ''
        }
        ScrollView {
            visible:!editor;Layout.fillWidth:true;Layout.fillHeight:true;clip:true
            GridLayout {
                width:parent.width;columns:2;columnSpacing:12;rowSpacing:12
                Repeater {
                    model:stateData.groups || []
                    ColumnLayout {
                        required property string modelData
                        Layout.fillWidth:true;Layout.alignment:Qt.AlignTop;Layout.preferredWidth:app.width/2-12
                        Label {text:modelData.toUpperCase();color:theme.accent;font.pixelSize:12}
                        Repeater {
                            model:app.tasks(modelData)
                            Rectangle {
                                required property var modelData
                                Layout.fillWidth:true;implicitHeight:84;radius:8
                                color:selectedId===modelData.id?theme.surface:theme.background
                                border.color:selectedId===modelData.id?theme.accent:theme.border
                                Column {anchors.fill:parent;anchors.margins:8;spacing:4
                                    Text {width:parent.width;text:(modelData.done?'✓  ':'○  ')+modelData.title;font.strikeout:modelData.done;color:theme.text;elide:Text.ElideRight}
                                    Text {width:parent.width;text:(modelData.status || 'ready')+' · '+(modelData.assignee || 'unassigned')+' · '+(modelData.verification && modelData.verification.length ? 'last check: '+modelData.verification[modelData.verification.length-1].outcome : 'unverified');color:theme.accent;font.pixelSize:11;elide:Text.ElideRight}
                                    Text {width:parent.width;text:(modelData.due?(modelData.period==='once'?modelData.due:modelData.period):'No due date')+(modelData.repeat.unit?' · ↻':'')+(modelData.snoozedUntil?' · snoozed':'');color:theme.muted;font.pixelSize:11;elide:Text.ElideRight}
                                }
                                MouseArea {anchors.fill:parent;onClicked:{selectedId=modelData.id;app.forceActiveFocus();}onDoubleClicked:app.action('toggle')}
                            }
                        }
                        Label {visible:app.tasks(modelData).length===0;text:'No tasks';color:theme.muted}
                    }
                }
            }
        }
        ScrollView {
            visible:editor==='edit';Layout.fillWidth:true;Layout.fillHeight:true;clip:true
            ColumnLayout {
                width:app.width-20
                Label {text:draftId?'Read later · save before closing tab':editing && editing.id?'Edit item':'New item';color:theme.text}
                TextField {color:theme.text;placeholderTextColor:theme.muted;id:titleInput;Layout.fillWidth:true;placeholderText:'Title (required)'}
                Label {text:'Group';color:theme.muted}
                ComboBox {id:groupInput;Layout.fillWidth:true;editable:true;model:stateData.groups || [];inputMethodHints:Qt.ImhNoPredictiveText}
                TextField {color:theme.text;placeholderTextColor:theme.muted;id:tagsInput;Layout.fillWidth:true;placeholderText:'Tags, comma separated'}
                TextField {color:theme.text;placeholderTextColor:theme.muted;id:dueInput;Layout.fillWidth:true;placeholderText:'Due YYYY-MM-DD (Quick is today)'}
                RowLayout {
                    ComboBox {id:repeatUnit;model:['none','days','weeks','months','years','days-of-the-week','days-of-the-month']}
                    TextField {color:theme.text;placeholderTextColor:theme.muted;id:repeatValue;Layout.fillWidth:true;placeholderText:'Interval or comma-separated day numbers'}
                }
                Label {text:'Reference';color:theme.muted}
                RowLayout {
                    TextField {color:theme.text;placeholderTextColor:theme.muted;id:planInput;Layout.fillWidth:true;placeholderText:editing && editing.kind==='reading'?'Reference · https://…':'Reference · Obsidian vault path'}
                    Button {text:'Find note';visible:!editing || editing.kind!=='reading';onClicked:{request({action:'plans'});planPicker.visible=true;}}
                }
                ColumnLayout {
                    id:planPicker;visible:false;Layout.fillWidth:true
                    TextField {color:theme.text;placeholderTextColor:theme.muted;id:planFilter;placeholderText:'Fuzzy file search';Layout.fillWidth:true}
                    Repeater {
                        model:plans.filter(p=>{let i=0;for(const c of p.toLowerCase())if(c===planFilter.text.toLowerCase()[i])i++;return i===planFilter.text.length;}).slice(0,15)
                        Button {required property string modelData;text:modelData;Layout.fillWidth:true;onClicked:{planInput.text=modelData;planPicker.visible=false;}}
                    }
                }
                RowLayout {
                    CheckBox {id:notifyInput;text:'Remind before due';checked:true}
                    TextField {id:leadValue;Layout.preferredWidth:60;text:'1';validator:IntValidator {bottom:0;top:10000}}
                    ComboBox {id:leadUnit;model:['hours','days','weeks'];currentIndex:1}
                }
                RowLayout {
                    Button {text:draftId?'Save and close tab':'Save';enabled:titleInput.text.trim()!=='';onClicked:saveEditor()}
                    Button {text:'Cancel';onClicked:cancelEditor()}
                }
            }
        }
        SnoozePicker {visible:editor==='snooze';Layout.fillWidth:true;ink:theme.text;accent:theme.accent;onChosen:until=>{action('snooze',{until:until});}}
        Button {visible:editor==='snooze';text:'Cancel';onClicked:{editor='';app.forceActiveFocus();}}
        Label {visible:!!errorMessage || !!stateData.message;text:errorMessage || stateData.message || '';color:theme.accent;wrapMode:Text.Wrap;Layout.fillWidth:true}
    }
    function accepted() {draftId='';editor='';app.forceActiveFocus();}
}
