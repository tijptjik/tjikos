import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: picker
    property color ink: '#e0def4'
    property color accent: '#ff53a6'
    property bool opened: false
    property bool calendarOpen: false
    property date month: new Date(new Date().getFullYear(), new Date().getMonth(), 1)
    property real navigationX: NaN
    property var navigationTarget: null
    spacing: 8
    enabled: opened
    onOpenedChanged: {
        calendarOpen=false;navigationTarget=null;navigationX=NaN;
        if(opened)Qt.callLater(()=>options.itemAt(0).forceActiveFocus(Qt.TabFocusReason));
    }
    function focusButtons() {
        const buttons=[];
        for(let i=0;i<options.count;i++)buttons.push(options.itemAt(i));
        if(calendarOpen) {
            buttons.push(previousMonth,nextMonth);
            for(let i=0;i<days.count;i++)if(days.itemAt(i).enabled)buttons.push(days.itemAt(i));
        }
        return buttons.concat([closeButton]);
    }
    function moveFocus(button, direction) {
        navigationTarget=null;navigationX=NaN;
        const buttons=focusButtons();
        buttons[(buttons.indexOf(button)+direction+buttons.length)%buttons.length].forceActiveFocus(Qt.TabFocusReason);
    }
    function arrowFocus(button, dx, dy) {
        const origin=button.mapToItem(picker,button.width/2,button.height/2);
        if(navigationTarget!==button || !isFinite(navigationX))navigationX=origin.x;
        const candidates=focusButtons().filter(b=>b!==button && b.enabled && b.visible).map(b=>{
            const point=b.mapToItem(picker,b.width/2,b.height/2);
            return {button:b,x:point.x,y:point.y};
        }).filter(item=>dy ? dy*(item.y-origin.y)>1 :
            dx*(item.x-origin.x)>1 && Math.abs(item.y-origin.y)<(item.button.height+button.height)/2);
        candidates.sort((a,b)=>dy ? Math.abs(a.y-origin.y)-Math.abs(b.y-origin.y) || Math.abs(a.x-navigationX)-Math.abs(b.x-navigationX) :
            Math.abs(a.x-origin.x)-Math.abs(b.x-origin.x));
        if(!candidates.length)return;
        const target=candidates[0];
        if(dx)navigationX=target.x;
        navigationTarget=target.button;
        target.button.forceActiveFocus(Qt.TabFocusReason);
    }
    component SnoozeButton: Button {
        id:control
        property color tint: picker.accent
        implicitHeight:34;padding:8
        focusPolicy:Qt.StrongFocus
        Keys.onTabPressed:event=>{picker.moveFocus(control,event.modifiers & Qt.ShiftModifier?-1:1);event.accepted=true;}
        Keys.onBacktabPressed:event=>{picker.moveFocus(control,-1);event.accepted=true;}
        Keys.onLeftPressed:event=>{picker.arrowFocus(control,-1,0);event.accepted=true;}
        Keys.onRightPressed:event=>{picker.arrowFocus(control,1,0);event.accepted=true;}
        Keys.onUpPressed:event=>{picker.arrowFocus(control,0,-1);event.accepted=true;}
        Keys.onDownPressed:event=>{picker.arrowFocus(control,0,1);event.accepted=true;}
        Keys.onReturnPressed:event=>{if(!event.isAutoRepeat)clicked();event.accepted=true;}
        Keys.onEnterPressed:event=>{if(!event.isAutoRepeat)clicked();event.accepted=true;}
        Keys.onEscapePressed:event=>{picker.cancelled();event.accepted=true;}
        contentItem:Text {text:control.text;color:control.tint;font.pixelSize:13;font.weight:Font.Medium;horizontalAlignment:Text.AlignHCenter;verticalAlignment:Text.AlignVCenter}
        background:Rectangle {
            radius:7
            color:Qt.alpha(control.tint,control.down?0.25:control.hovered || control.activeFocus?0.17:0.08)
            border.width:control.activeFocus?2:1
            border.color:Qt.alpha(control.tint,control.activeFocus?1:control.hovered?0.5:0.2)
            opacity:control.enabled?1:0.3
        }
    }
    signal chosen(string until)
    signal cancelled()
    function choose(label) {
        let d=new Date();
        const hours={Afternoon:14,Evening:18,Night:21};
        if(hours[label]) {d.setHours(hours[label],0,0,0);if(d<=new Date())d.setDate(d.getDate()+1);}
        else {d.setHours(9,0,0,0);const weekday=d.getDay();d.setDate(d.getDate()+(label==='Tomorrow'?1:label==='Weekend'?((6-weekday+7)%7 || 7):((8-weekday)%7 || 7)));}
        chosen(d.toISOString());
    }
    RowLayout {
        Layout.fillWidth:true
        Label {text:'Snooze until';color:picker.ink;opacity:0.65;font.pixelSize:11;Layout.fillWidth:true}
        SnoozeButton {
            id:closeButton;objectName:'snooze-close';text:'×';tint:picker.ink
            implicitWidth:28;implicitHeight:28;padding:2
            Accessible.name:'Cancel snooze'
            ToolTip.visible:hovered;ToolTip.text:'Cancel · Esc'
            onClicked:picker.cancelled()
        }
    }
    GridLayout {
        Layout.fillWidth:true;columns:width>=300?3:2;columnSpacing:6;rowSpacing:6
        Repeater {
            id:options
            model:['Afternoon','Evening','Night','Tomorrow','Weekend','Next Week','Later']
            SnoozeButton {
                required property string modelData
                required property int index
                objectName:'snooze-'+modelData
                text:modelData==='Later'?'Later…':modelData
                tint:['#f2ce98','#ebadbd','#c4b2e7',picker.accent,'#9dc6d8','#9dc6d8',picker.accent][index]
                Layout.fillWidth:true;Layout.minimumWidth:0;Layout.preferredWidth:1
                Layout.columnSpan:modelData==='Later'?parent.columns:1
                onClicked: {
                    if(modelData==='Later') {
                        picker.calendarOpen=!picker.calendarOpen;
                        if(picker.calendarOpen)Qt.callLater(()=>nextMonth.forceActiveFocus(Qt.TabFocusReason));
                    } else picker.choose(modelData);
                }
            }
        }
    }
    ColumnLayout {
        visible:picker.calendarOpen;Layout.fillWidth:true
        RowLayout {
            SnoozeButton {id:previousMonth;text:'‹';Accessible.name:'Previous month';onClicked:picker.month=new Date(picker.month.getFullYear(),picker.month.getMonth()-1,1)}
            Label {Layout.fillWidth:true;text:Qt.formatDate(picker.month,'MMMM yyyy');color:picker.ink;horizontalAlignment:Text.AlignHCenter}
            SnoozeButton {id:nextMonth;text:'›';Accessible.name:'Next month';onClicked:picker.month=new Date(picker.month.getFullYear(),picker.month.getMonth()+1,1)}
        }
        GridLayout {
            columns:7;Layout.fillWidth:true;columnSpacing:2;rowSpacing:2
            Repeater {model:['Mon','Tue','Wed','Thu','Fri','Sat','Sun'];Label {required property string modelData;required property int index;text:modelData;color:index>=5?picker.accent:picker.ink;Layout.fillWidth:true;horizontalAlignment:Text.AlignHCenter}}
            Repeater {
                id:days
                model:42
                SnoozeButton {
                    required property int index
                    readonly property date dateValue:new Date(picker.month.getFullYear(),picker.month.getMonth(),index-((picker.month.getDay()+6)%7)+1,9)
                    Layout.fillWidth:true;Layout.minimumWidth:0;Layout.preferredWidth:32;implicitHeight:30
                    text:dateValue.getMonth()===picker.month.getMonth()?dateValue.getDate():''
                    enabled:text!=='' && dateValue>new Date()
                    Accessible.name:Qt.formatDate(dateValue,'dddd, d MMMM yyyy')
                    contentItem:Text {text:parent.text;color:index%7>=5?picker.accent:picker.ink;opacity:parent.enabled?1:.3;horizontalAlignment:Text.AlignHCenter;verticalAlignment:Text.AlignVCenter}
                    onClicked:picker.chosen(dateValue.toISOString())
                }
            }
        }
    }
}
