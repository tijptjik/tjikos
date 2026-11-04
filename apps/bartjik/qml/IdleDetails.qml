import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Column {
    id:details
    required property var popup
    readonly property var theme:popup.theme
    readonly property var info:popup.idlePanel?(popup.panelData || ({})):({})
    readonly property bool busy:popup.feedback.busy===true
    property int openChoices:0
    readonly property bool needsKeyboard:openChoices>0
    property var draft:({})
    property string revision:''
    property bool dirty:false
    property double clock:Date.now()/1000
    property bool confirming:false
    property bool previouslyPinned:false
    readonly property int remaining:Math.max(0,Math.ceil((info.deadline || 0)-clock))
    readonly property var delayOptions:[0,15,30,60,120,180,300,600,900,1800,3600,7200,14400,28800,86400]
    spacing:16
    function durationLabel(seconds){return seconds===0?'Never':seconds<60?seconds+' seconds':seconds<3600?(seconds/60)+(seconds===60?' minute':' minutes'):(seconds/3600)+(seconds===3600?' hour':' hours');}
    function timeLeft(seconds){const minutes=Math.ceil(seconds/60);return minutes>=60?Math.floor(minutes/60)+'h '+(minutes%60)+'m':minutes+' min';}
    function reset(){draft=Object.assign({},info.delays || {});revision=info.revision || '';dirty=false;}
    function edit(key,value){draft=Object.assign({},draft,{[key]:value});dirty=JSON.stringify(draft)!==JSON.stringify(info.delays || {});}
    function act(name,value){popup.action(name,String(value));}
    function cancelPower(){confirming=false;popup.pinned=previouslyPinned;}
    function inspect(){return {inhibited:info.inhibited,duration:info.duration,remaining:remaining,delays:draft,dirty:dirty,confirming:confirming};}
    onInfoChanged:{clock=Date.now()/1000;if(!dirty)reset();}
    Component.onCompleted:reset()
    Timer {interval:1000;running:details.visible && details.popup.idlePanel;repeat:true;onTriggered:details.clock=Date.now()/1000}
    Connections {
        target:popup
        function onOpenedChanged(){if(!popup.opened){if(details.confirming)details.cancelPower();details.reset();}}
        function onIdlePanelChanged(){if(!popup.idlePanel && details.confirming)details.cancelPower();}
        function onFeedbackChanged(){if(!popup.feedback.busy && !popup.feedback.error && popup.feedback.message==='Idle delays saved')details.reset();}
    }
    component Label: Text {font.family:details.theme.font;font.pixelSize:12;color:details.theme.text;textFormat:Text.PlainText}
    component Choice: ComboBox {
        id:control;required property var values;required property var selection
        function syncSelection(){currentIndex=values.findIndex(v=>v.value===selection);}
        onValuesChanged:Qt.callLater(syncSelection)
        onSelectionChanged:Qt.callLater(syncSelection)
        Component.onCompleted:syncSelection()
        model:values.map(v=>v.label);implicitHeight:34;enabled:!details.busy
        font.family:details.theme.font;font.pixelSize:12
        background:Rectangle {radius:7;color:details.theme.surface;border.width:control.activeFocus?1:0;border.color:details.theme.accent}
        contentItem:Label {text:control.displayText;verticalAlignment:Text.AlignVCenter;leftPadding:10;rightPadding:24;elide:Text.ElideRight}
        indicator:Label {x:parent.width-20;y:9;text:'⌄';color:details.theme.muted}
        delegate:ItemDelegate {
            required property string modelData
            required property int index
            width:control.width;height:32
            background:Rectangle {color:parent.highlighted?details.theme.surface:details.theme.background}
            contentItem:Label {text:parent.modelData;elide:Text.ElideRight;verticalAlignment:Text.AlignVCenter}
            highlighted:control.highlightedIndex===index
        }
        popup:Popup {
            y:control.height+4;width:control.width;padding:4
            onVisibleChanged:details.openChoices=Math.max(0,details.openChoices+(visible?1:-1))
            implicitHeight:Math.min(240,contentItem.implicitHeight+8)
            background:Rectangle {radius:8;color:details.theme.background;border.color:details.theme.border}
            contentItem:ListView {clip:true;implicitHeight:contentHeight;model:control.popup.visible?control.delegateModel:null;currentIndex:control.highlightedIndex;ScrollBar.vertical:ScrollBar {}}
        }
    }
    Column {
        width:parent.width;spacing:6
        Switch {
            id:awake;objectName:'idle-enabled';width:parent.width;height:34;padding:0;enabled:!details.busy;focusPolicy:Qt.NoFocus
            checked:details.info.inhibited===true
            contentItem:Label {text:'Keep awake';font.pixelSize:14;verticalAlignment:Text.AlignVCenter;rightPadding:54}
            indicator:Rectangle {
                x:awake.width-width;y:(awake.height-height)/2;width:38;height:22;radius:11
                color:awake.checked?details.theme.accent:details.theme.surface;border.width:awake.checked?0:1;border.color:details.theme.border
                Rectangle {x:awake.checked?18:3;y:3;width:16;height:16;radius:8;color:awake.checked?details.theme.background:details.theme.text;Behavior on x {NumberAnimation {duration:120}}}
            }
            onClicked:{details.act('enabled',checked?'on':'off');checked=Qt.binding(()=>details.info.inhibited===true);}
        }
        Label {
            objectName:'idle-state';width:parent.width;wrapMode:Text.Wrap;font.pixelSize:11;color:details.theme.muted
            text:details.info.inhibited && details.info.deadline?(details.remaining?'Resumes automatically in '+details.timeLeft(details.remaining):'Resuming automatic idle handling…'):''
            visible:!!text
        }
    }
    RowLayout {
        width:parent.width;spacing:16
        Label {text:'Keep awake for';Layout.fillWidth:true}
        Choice {objectName:'idle-duration';Layout.preferredWidth:200;selection:details.info.duration || 0
            values:[{value:1800,label:'30 minutes'},{value:3600,label:'1 hour'},{value:7200,label:'2 hours'},{value:0,label:'Until switched off'}]
            onActivated:index=>{details.act('duration',values[index].value);syncSelection();}}
    }
    Rectangle {width:parent.width;height:1;color:details.theme.surface}
    Column {
        width:parent.width;spacing:10
        Label {text:'When inactive';font.pixelSize:13}
        Label {visible:details.info.gamePolicy===true;width:parent.width;wrapMode:Text.Wrap;text:'Using your game-mode idle policy';font.pixelSize:11;color:details.theme.muted}
        Repeater {
            model:[{key:'dim',label:'Dim screens'},{key:'lock',label:'Lock session'},{key:'screenOff',label:'Turn screens off'},{key:'suspend',label:'Sleep'}]
            delegate:RowLayout {
                required property var modelData
                width:details.width;spacing:16
                Label {text:modelData.label;Layout.fillWidth:true}
                Choice {objectName:'idle-delay-'+modelData.key;Layout.preferredWidth:164;enabled:!details.busy && details.info.editable===true
                    selection:details.draft[modelData.key] ?? 0
                    values:Array.from(new Set(details.delayOptions.concat([selection]))).sort((a,b)=>a-b).map(v=>({value:v,label:details.durationLabel(v)}))
                    onActivated:index=>details.edit(modelData.key,values[index].value)}
            }
        }
        RowLayout {
            width:parent.width
            Label {text:details.dirty?'Unsaved changes':details.info.inhibited?'Used when Keep awake is off':'';font.pixelSize:11;color:details.theme.muted;Layout.fillWidth:true;wrapMode:Text.Wrap}
            PanelButton {objectName:'idle-reset';theme:details.theme;showLabel:true;text:'Reset';enabled:details.dirty && !details.busy;onClicked:details.reset()}
            PanelButton {objectName:'idle-apply';theme:details.theme;showLabel:true;text:'Apply';enabled:details.dirty && !details.busy && details.info.editable===true;onClicked:details.act('delays',JSON.stringify({revision:details.revision,delays:details.draft}))}
        }
    }
    Rectangle {width:parent.width;height:1;color:details.theme.surface}
    Row {
        anchors.horizontalCenter:parent.horizontalCenter;spacing:8
        PanelButton {objectName:'idle-lock';theme:details.theme;showLabel:true;text:'Lock now';enabled:!details.busy;onClicked:details.act('lock','')}
        PanelButton {objectName:'idle-suspend';theme:details.theme;showLabel:true;text:'Sleep now';enabled:!details.busy;onClicked:details.act('suspend','')}
        PanelButton {objectName:'idle-poweroff';theme:details.theme;showLabel:true;text:'Power off now';enabled:!details.busy;onClicked:{details.previouslyPinned=details.popup.pinned;details.popup.pinned=true;details.confirming=true;}}
    }
    Label {objectName:'idle-feedback';width:parent.width;wrapMode:Text.Wrap;font.pixelSize:11
        text:details.popup.feedback.error?details.popup.feedback.message || '':details.info.error || ''
        visible:!!text;color:details.theme.accent}
    PowerConfirmation {
        theme:details.theme;screen:details.popup.panelScreen || null
        visible:details.confirming && details.popup.idlePanel && details.popup.opened
        onCancelled:details.cancelPower()
        onAccepted:{details.cancelPower();details.act('poweroff','confirm');}
    }
}
