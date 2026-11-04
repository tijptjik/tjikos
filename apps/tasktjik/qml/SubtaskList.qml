import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "DateCode.js" as DateCode

ColumnLayout {
    id:root
    property var task
    property var theme
    property bool busy:false
    property bool showBlocked:false
    property string view:'all'
    property string today:Qt.formatDate(new Date(),'yyyy-MM-dd')
    property string focusedId:''
    signal toggle(string id)
    signal add()
    signal navigate(int direction,int modifiers)
    signal focusReached(string id)
    function navigationItems() {const rows=[];for(let i=0;i<steps.count;i++){const row=steps.itemAt(i);if(row.visible && row.actionable)rows.push(row);}return rows;}
    spacing:2
    Repeater {
        id:steps
        model:root.task ? (root.task.workflowRows || []).filter(r=>root.showBlocked || (!r.blocked && r.status!=='blocked')) : []
        Button {
            id:step
            required property var modelData
            objectName:'subtask-'+modelData.id
            Layout.fillWidth:true
            padding:0;leftPadding:0;rightPadding:0;topPadding:4;bottomPadding:4
            readonly property bool actionable:!['flow','group'].includes(modelData.kind) && (modelData.cancelled===true || !modelData.blocked && !['cancelled','blocked'].includes(modelData.status))
            readonly property bool chosen:modelData.status==='done' && modelData.kind!=='reminder' && (!!modelData.blockId || modelData.kind==='task' && (root.task.workflowRows || []).some(r=>r.kind==='group' && (r.items || []).some(c=>c.id===modelData.id)))
            readonly property bool flowPicked:modelData.kind==='flow' && (root.task.workflowRows || []).some(r=>r.blockId===modelData.id && r.kind!=='reminder' && r.status==='done')
            readonly property color dotColor:modelData.kind==='exit'?'#eb6f92':modelData.kind==='reminder'?'#f6c177':root.theme.text
            enabled:!root.busy
            focusPolicy:Qt.NoFocus
            readonly property bool selected:root.focusedId===modelData.id
            onActiveFocusChanged:if(activeFocus && actionable)root.focusReached(modelData.id)
            Accessible.name:modelData.title+(modelData.kind==='exit'?', exit option':modelData.kind==='reminder'?', reminder':'')+', '+modelData.status
            Keys.onDownPressed:event=>{root.navigate(1,event.modifiers);event.accepted=true;}
            Keys.onUpPressed:event=>{root.navigate(-1,event.modifiers);event.accepted=true;}
            onClicked:if(actionable)root.focusReached(modelData.id)
            Keys.onReturnPressed:event=>{if(actionable && !event.isAutoRepeat)root.toggle(modelData.id);event.accepted=true;}
            Keys.onEnterPressed:event=>{if(actionable && !event.isAutoRepeat)root.toggle(modelData.id);event.accepted=true;}
            background:Rectangle {radius:4;color:step.hovered?root.theme.surface:'transparent';border.color:'transparent'}
            TaskToolTip {theme:root.theme;visible:step.hovered
            text:(modelData.kind==='exit'?'Ends the workflow · ':modelData.kind==='reminder'?'Reminder · ':'')+(modelData.status==='snoozed'?'Snoozed until ':modelData.status==='blocked'?'Waiting for required steps · ':'')+(modelData.due || '')}
            contentItem:RowLayout {
                spacing:8
                Button {
                    id:marker
                    objectName:'subtask-complete-'+step.modelData.id
                    Layout.preferredWidth:30;Layout.preferredHeight:24;Layout.alignment:Qt.AlignTop
                    padding:0;focusPolicy:Qt.NoFocus;enabled:step.actionable && step.modelData.status!=='cancelled' && !root.busy
                    Accessible.name:step.modelData.status==='done'?'Reopen subtask':'Complete subtask'
                    onClicked:{root.focusReached(step.modelData.id);root.toggle(step.modelData.id);}
                    background:Item {}
                    contentItem:Item {
                        Label {
                            objectName:'subtask-date-'+step.modelData.id
                            anchors.left:parent.left;anchors.right:parent.right;anchors.verticalCenter:parent.verticalCenter
                            visible:!step.selected && !marker.hovered
                            horizontalAlignment:Text.AlignHCenter
                            text:DateCode.dateCode(step.modelData.due,root.today,root.view)
                            color:step.modelData.due && step.modelData.due<root.today && step.modelData.status==='ready'?'#eb6f92':root.theme.muted
                            font.family:'monospace';font.pixelSize:11;font.weight:Font.DemiBold
                            font.underline:step.modelData.status==='snoozed'
                        }
                        Label {
                            objectName:'subtask-chevron-'+step.modelData.id
                            anchors.centerIn:parent;text:'›';font.pixelSize:22;color:root.theme.accent
                            visible:step.selected && !marker.hovered
                        }
                        Rectangle {
                            anchors.centerIn:parent;width:14;height:14;radius:3
                            visible:marker.hovered
                            color:step.modelData.status==='done'?root.theme.accent:'transparent'
                            border.width:1;border.color:root.theme.accent
                            Text {anchors.centerIn:parent;text:'✓';font.pixelSize:11;color:root.theme.background;visible:step.modelData.status==='done'}
                        }
                    }
                }
                Item {Layout.preferredWidth:step.modelData.depth*12;visible:step.modelData.depth>0}
                Item {
                    Layout.preferredWidth:16;Layout.preferredHeight:18;Layout.alignment:Qt.AlignTop
                    Rectangle {
                        anchors.centerIn:parent
                        readonly property bool checking:false
                        width:checking?14:5;height:width;radius:checking?3:width/2
                        visible:!['flow','group'].includes(step.modelData.kind)
                        color:checking?(step.modelData.status==='done'?step.dotColor:'transparent'):step.dotColor
                        opacity:['blocked','cancelled'].includes(step.modelData.status)?0.35:step.modelData.status==='done'?0.55:1
                        border.color:step.dotColor;border.width:checking?1:0
                        Behavior on width {NumberAnimation {duration:100}}
                        Text {anchors.centerIn:parent;text:'✓';visible:parent.checking && step.modelData.status==='done';font.pixelSize:11;color:root.theme.background}
                    }
                    Canvas {
                        anchors.centerIn:parent;width:16;height:16;visible:['flow','group'].includes(step.modelData.kind)
                        property color ink:step.modelData.status==='blocked'?root.theme.muted:root.theme.accent
                        onInkChanged:requestPaint()
                        onPaint:{const c=getContext('2d');c.reset();c.strokeStyle=ink;c.lineWidth=1.4;c.lineCap='round';c.beginPath();c.moveTo(3,8);c.lineTo(7,8);c.lineTo(7,3);c.lineTo(13,3);c.moveTo(7,8);c.lineTo(7,13);c.lineTo(13,13);c.stroke();c.fillStyle=ink;for(const p of [[3,8],[13,3],[13,13]]){c.beginPath();c.arc(p[0],p[1],1.5,0,Math.PI*2);c.fill();}}
                    }
                }
                Label {
                    objectName:'subtask-title-'+step.modelData.id
                    text:step.modelData.title;textFormat:Text.PlainText
                    Layout.fillWidth:true;wrapMode:Text.Wrap
                    color:step.selected?root.theme.accent:['cancelled','snoozed','blocked'].includes(step.modelData.status)?root.theme.muted:root.theme.text
                    font.pixelSize:13;font.bold:step.chosen || ['flow','group'].includes(step.modelData.kind);font.strikeout:step.modelData.status==='cancelled' || step.flowPicked || step.modelData.status==='done' && !step.chosen
                }
            }
        }
    }
    AddSubtaskButton {theme:root.theme;objectName:'subtask-add';Layout.leftMargin:33;enabled:!root.busy;onClicked:root.add();Accessible.name:'Add subtask at end of list'}
}
