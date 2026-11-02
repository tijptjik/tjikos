import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell

FloatingWindow {
    id:settings
    property alias panelItem:panel
    property var theme
    property var stateData:({groups:[],tasks:[]})
    property bool busy:false
    property string errorMessage:''
    property string tab:'general'
    property int doubleDInterval:350
    signal doubleDIntervalSelected(int value)
    property string domainSort:'manual'
    property var domainGroups:[]
    signal domainSortSelected(string value)
    signal moveDomain(string group,int direction)
    property string projectSort:'first-due'
    property var pending:null
    property var dragRows:[]
    property bool dragging:false
    property real dragY:0
    property string editingGroup:''
    property string searchText:''
    signal operation(var value)
    signal editTemplate(var task)
    signal newTemplate()
    signal projectSortSelected(string value)
    title:'Tasktjik Settings'
    visible:false
    implicitWidth:900;implicitHeight:660
    minimumSize:Qt.size(620,440)
    color:theme.background
    function open(){visible=true;}
    function close(){visible=false;}
    onClosed:visible=false
    function matches(text){return !searchText || text.toLowerCase().includes(searchText.toLowerCase());}
    function saveGroup(source,target){if(target.trim() && target.trim()!==source)operation(Object.assign(command('group-rename',source,'',null,false),{target:target.trim()}));editingGroup='';}
    function groupName(name) {return name==='All'?'Inbox':name;}
    function command(action, source, target, members, scoped) {
        return {action:action,source:source,target:target,groupsRevision:stateData.groupsRevision || 0,
            members:(members || stateData.tasks.filter(t=>t.group===source)).map(t=>({id:t.id,revision:t.revision})),scoped:!!scoped};
    }
    function rename(source, members) {
        editingGroup=source;
    }
    function merge(source,target) {
        if(source===target || source==='All')return;
        pending=command('group-merge',source,target);
        confirmText.text='Move all '+pending.members.length+' tasks from “'+groupName(source)+'” into “'+groupName(target)+'” and remove “'+groupName(source)+'”?';
        confirmDialog.open();
    }
    function remove(source) {
        pending=command('group-remove',source,'All');
        confirmText.text='Remove “'+groupName(source)+'”? Its '+pending.members.length+' tasks will move to Inbox.';
        confirmDialog.open();
    }

    component ActionButton: Button {
        id:button
        implicitHeight:32;leftPadding:12;rightPadding:12
        contentItem:Text {text:button.text;font.pixelSize:13;color:button.enabled?settings.theme.text:settings.theme.muted;verticalAlignment:Text.AlignVCenter;horizontalAlignment:Text.AlignHCenter}
        background:Rectangle {radius:6;color:button.down?Qt.alpha(settings.theme.accent,0.18):button.hovered?Qt.alpha(settings.theme.accent,0.1):'transparent';border.color:button.visualFocus?settings.theme.accent:Qt.alpha(settings.theme.border,0.5)}
    }
    component IconAction: Button {
        id:button
        property string glyph:'close'
        implicitWidth:30;implicitHeight:30;padding:0
        contentItem:Item {Canvas {
            anchors.centerIn:parent;width:16;height:16
            property color ink:button.enabled?(button.hovered?settings.theme.accent:settings.theme.muted):Qt.alpha(settings.theme.muted,0.35)
            onInkChanged:requestPaint()
            onPaint:{const c=getContext('2d');c.reset();c.strokeStyle=ink;c.lineWidth=1.4;c.lineCap='round';c.lineJoin='round';c.beginPath();
                if(button.glyph==='edit'){c.moveTo(3,10);c.lineTo(10,3);c.lineTo(13,6);c.lineTo(6,13);c.lineTo(3,13);c.closePath();c.moveTo(8,5);c.lineTo(11,8);}
                else if(button.glyph==='remove'){c.moveTo(3,4);c.lineTo(13,4);c.moveTo(6,2);c.lineTo(10,2);c.moveTo(4,4);c.lineTo(5,14);c.lineTo(11,14);c.lineTo(12,4);}
                else {c.moveTo(4,4);c.lineTo(12,12);c.moveTo(12,4);c.lineTo(4,12);}c.stroke();}
        }}
        background:Rectangle {radius:6;color:button.down?Qt.alpha(settings.theme.accent,0.18):button.hovered?Qt.alpha(settings.theme.accent,0.09):'transparent';border.color:button.visualFocus?settings.theme.accent:'transparent'}
        TaskToolTip {theme:settings.theme;visible:button.hovered;text:button.Accessible.name}
    }
    Timer {interval:16;repeat:true;running:settings.dragging;onTriggered:{const flick=groupScroll.contentItem;const delta=settings.dragY<32?-7:settings.dragY>groupScroll.height-32?7:0;flick.contentY=Math.max(0,Math.min(flick.contentHeight-flick.height,flick.contentY+delta));}}
    Pane {
        id:panel;anchors.fill:parent;padding:0;focus:true
        palette.windowText:theme.text;palette.text:theme.text;palette.buttonText:theme.text
        palette.window:theme.background;palette.base:theme.background;palette.button:theme.surface
        palette.highlight:theme.accent;palette.highlightedText:theme.background
        background:Rectangle {color:theme.background}
        Keys.onEscapePressed:{if(settings.editingGroup)settings.editingGroup='';else settings.close();}
        ColumnLayout {
            anchors.fill:parent;spacing:0
            RowLayout {
                Layout.fillWidth:true;Layout.preferredHeight:52;spacing:12
                Label {text:'Settings';color:theme.text;font.pixelSize:15;font.weight:Font.DemiBold;Layout.leftMargin:20;Layout.fillWidth:true}
                IconAction {Accessible.name:'Close settings';Layout.rightMargin:14;onClicked:settings.close()}
            }
            Rectangle {Layout.fillWidth:true;implicitHeight:1;color:Qt.alpha(theme.border,0.45)}
            RowLayout {
                Layout.fillWidth:true;Layout.fillHeight:true;spacing:0
                Rectangle {
                    Layout.preferredWidth:190;Layout.fillHeight:true;color:Qt.darker(theme.background,1.18)
                    ColumnLayout {
                        anchors.fill:parent;anchors.margins:12;spacing:7
                        TextField {
                            objectName:'settings-search';Layout.fillWidth:true;implicitHeight:34
                            placeholderText:'Search settings…';font.pixelSize:12;color:theme.text;placeholderTextColor:theme.muted
                            padding:8;selectByMouse:true;onTextChanged:settings.searchText=text
                            background:Rectangle {radius:6;color:theme.surface;border.color:parent.activeFocus?theme.accent:'transparent'}
                        }
                        Item {implicitHeight:8}
                        Repeater {
                            model:[{key:'general',label:'General'},{key:'groups',label:'Groups'},{key:'templates',label:'Templates'}]
                            Button {
                                required property var modelData
                                Layout.fillWidth:true;implicitHeight:36;leftPadding:12;rightPadding:12
                                visible:settings.matches(modelData.label) || modelData.key==='general' && settings.matches('project sorting first last due')
                                onClicked:{settings.tab=modelData.key;settings.editingGroup='';}
                                contentItem:Text {text:parent.modelData.label;font.pixelSize:13;color:settings.tab===parent.modelData.key?theme.text:theme.muted;verticalAlignment:Text.AlignVCenter}
                                background:Rectangle {radius:6;color:settings.tab===parent.modelData.key?Qt.alpha(theme.accent,0.16):parent.hovered?Qt.alpha(theme.accent,0.07):'transparent';border.color:parent.visualFocus?theme.accent:'transparent'}
                            }
                        }
                        Item {Layout.fillHeight:true}
                        Label {text:'Tasktjik';color:theme.muted;font.pixelSize:11;Layout.leftMargin:12;Layout.bottomMargin:4}
                    }
                }
                Rectangle {Layout.fillHeight:true;implicitWidth:1;color:Qt.alpha(theme.border,0.35)}
                Item {
                    Layout.fillWidth:true;Layout.fillHeight:true
                    ColumnLayout {
                        anchors.fill:parent;anchors.margins:28;spacing:20
                        RowLayout {
                            Layout.fillWidth:true
                            Label {text:settings.tab==='groups'?'Groups':settings.tab==='templates'?'Templates':'General';color:theme.text;font.pixelSize:21;Layout.fillWidth:true}
                            ActionButton {objectName:'settings-new';visible:settings.tab!=='general';text:settings.tab==='groups'?'+ Group':'+ Template';enabled:!settings.busy;onClicked:{if(settings.tab==='templates'){settings.close();settings.newTemplate();}else {settings.pending=settings.command('group-add','','',[]);nameDialog.title='New group';nameInput.text='';nameDialog.open();nameInput.forceActiveFocus();}}}
                        }
                        ColumnLayout {
                            visible:settings.tab==='general';Layout.fillWidth:true;spacing:16
                            RowLayout {
                                Layout.fillWidth:true
                                Label {text:'Double-D delete interval (ms)';color:theme.text;Layout.fillWidth:true}
                                SpinBox {from:150;to:1000;stepSize:50;value:settings.doubleDInterval;editable:true;onValueModified:settings.doubleDIntervalSelected(value)}
                            }
                            RowLayout {
                                Layout.fillWidth:true
                                Label {text:'Domain order';color:theme.text;Layout.fillWidth:true}
                                ComboBox {objectName:'domain-sort-choice';model:['Manual','Alphabetical','Due (earliest)'];currentIndex:Math.max(0,['manual','alphabetical','due'].indexOf(settings.domainSort));onActivated:settings.domainSortSelected(['manual','alphabetical','due'][currentIndex])}
                            }
                            Label {Layout.fillWidth:true;wrapMode:Text.Wrap;text:'Order and columns stay fixed until you switch views. Arrange Manual order with the arrows in Groups.';color:theme.muted;font.pixelSize:12}
                            Label {text:'TASK LIST';font.pixelSize:11;font.letterSpacing:1;color:theme.muted}
                            RowLayout {
                                visible:settings.matches('project sorting first last due');Layout.fillWidth:true;spacing:20
                                ColumnLayout {
                                    Layout.fillWidth:true;spacing:7
                                    Label {text:'Project sorting';color:theme.text;font.pixelSize:14}
                                    Label {Layout.fillWidth:true;wrapMode:Text.Wrap;font.pixelSize:12;lineHeight:1.25;color:theme.muted;text:'Choose where projects appear in each group. First Due uses the earliest unfinished task’s due date; undated projects follow dated tasks.'}
                                }
                                ComboBox {
                                    id:projectChoice;objectName:'project-sort-choice';implicitWidth:145;implicitHeight:34
                                    model:['Always First','Always Last','First Due']
                                    currentIndex:Math.max(0,['first','last','first-due'].indexOf(settings.projectSort))
                                    Accessible.name:'Project sorting'
                                    onActivated:settings.projectSortSelected(['first','last','first-due'][currentIndex])
                                    contentItem:Text {text:projectChoice.displayText;color:theme.text;font.pixelSize:13;verticalAlignment:Text.AlignVCenter;leftPadding:10;rightPadding:24}
                                    background:Rectangle {radius:6;color:projectChoice.hovered?theme.surface:'transparent';border.color:projectChoice.visualFocus?theme.accent:Qt.alpha(theme.border,0.6)}
                                    popup:Popup {
                                        y:projectChoice.height+5;width:projectChoice.width;padding:4
                                        implicitHeight:contentItem.implicitHeight+8
                                        background:Rectangle {radius:7;color:theme.background;border.color:theme.border}
                                        contentItem:Column {Repeater {model:projectChoice.model;Button {required property string modelData;required property int index;width:projectChoice.width-8;height:32;onClicked:{projectChoice.currentIndex=index;projectChoice.activated(index);projectChoice.popup.close();}contentItem:Text {text:parent.modelData;color:theme.text;font.pixelSize:13;verticalAlignment:Text.AlignVCenter;leftPadding:8}background:Rectangle {radius:4;color:parent.hovered || parent.visualFocus?theme.surface:'transparent'}}}}
                                    }
                                }
                            }
                            Rectangle {Layout.fillWidth:true;implicitHeight:1;color:Qt.alpha(theme.border,0.35)}
                        }
                        Label {visible:settings.tab!=='general';Layout.fillWidth:true;wrapMode:Text.Wrap;font.pixelSize:12;color:theme.muted;text:settings.tab==='groups'?'Rename groups here, or drag a handle onto another group to merge.':'Reusable workflows for new tasks. Select a template to edit it.'}
                        KineticScrollView {
                            id:groupScroll;objectName:'settings-scroll'
                            visible:settings.tab!=='general';Layout.fillWidth:true;Layout.fillHeight:true;clip:true;contentWidth:availableWidth
                            ScrollBar.horizontal.policy:ScrollBar.AlwaysOff
                            ColumnLayout {
                                width:groupScroll.availableWidth;spacing:0
                                Repeater {
                                    model:settings.tab==='groups'?settings.domainGroups:[]
                                    Rectangle {
                                        id:groupRow;objectName:'settings-group-'+modelData
                                        required property string modelData
                                        visible:settings.matches(modelData)
                                        Layout.fillWidth:true;implicitHeight:48;radius:6;color:target.containsDrag?Qt.alpha(theme.accent,0.16):rowHover.hovered?Qt.alpha(theme.accent,0.04):'transparent'
                                        Component.onCompleted:settings.dragRows=settings.dragRows.concat([groupRow])
                                        Component.onDestruction:settings.dragRows=settings.dragRows.filter(r=>r!==groupRow)
                                        HoverHandler {id:rowHover}
                                        DropArea {id:target;anchors.fill:parent;keys:['tasktjik-group'];onDropped:drop=>{settings.merge(drop.source.groupName,groupRow.modelData);drop.acceptProposedAction();}}
                                        RowLayout {
                                            anchors.fill:parent;anchors.margins:4;spacing:8
                                            Item {
                                                Layout.preferredWidth:24;Layout.fillHeight:true
                                                Rectangle {
                                                    id:handle;objectName:'group-handle-'+groupRow.modelData;property string groupName:groupRow.modelData
                                                    width:24;height:36;color:dragMouse.drag.active?theme.surface:'transparent';radius:4
                                                    Drag.active:dragMouse.drag.active;Drag.keys:['tasktjik-group'];Drag.hotSpot.x:12;Drag.hotSpot.y:18
                                                    Canvas {anchors.centerIn:parent;width:12;height:16;onPaint:{const c=getContext('2d');c.reset();c.fillStyle=theme.muted;for(let x=3;x<=9;x+=6)for(let y=3;y<=13;y+=5){c.beginPath();c.arc(x,y,1,0,Math.PI*2);c.fill();}}}
                                                    MouseArea {id:dragMouse;anchors.fill:parent;enabled:groupRow.modelData!=='All' && !settings.busy;cursorShape:Qt.OpenHandCursor;drag.target:handle;onPositionChanged:mouse=>{if(drag.active){settings.dragging=true;settings.dragY=mapToItem(groupScroll,mouse.x,mouse.y).y;}}onReleased:{settings.dragging=false;handle.Drag.drop();handle.x=0;handle.y=0;}onCanceled:{settings.dragging=false;handle.x=0;handle.y=0;}}
                                                }
                                            }
                                            Label {visible:settings.editingGroup!==groupRow.modelData;text:settings.groupName(groupRow.modelData);Layout.fillWidth:true;color:theme.text;font.pixelSize:13;elide:Text.ElideRight;MouseArea {anchors.fill:parent;onDoubleClicked:settings.rename(groupRow.modelData,null);enabled:groupRow.modelData!=='All'}}
                                            TextField {id:groupEdit;visible:settings.editingGroup===groupRow.modelData;Layout.fillWidth:true;text:groupRow.modelData;color:theme.text;selectByMouse:true;onVisibleChanged:if(visible){text=groupRow.modelData;forceActiveFocus();selectAll();}onAccepted:settings.saveGroup(groupRow.modelData,text);Keys.onEscapePressed:settings.editingGroup='';background:Rectangle {radius:4;color:theme.surface;border.color:theme.accent}}
                                            ActionButton {visible:groupEdit.visible;text:'Save';enabled:!settings.busy;onClicked:settings.saveGroup(groupRow.modelData,groupEdit.text)}
                                            ActionButton {text:'↑';Accessible.name:'Move domain up';enabled:settings.domainGroups.indexOf(groupRow.modelData)>0;onClicked:settings.moveDomain(groupRow.modelData,-1)}
                                            ActionButton {text:'↓';Accessible.name:'Move domain down';enabled:settings.domainGroups.indexOf(groupRow.modelData)<settings.domainGroups.length-1;onClicked:settings.moveDomain(groupRow.modelData,1)}
                                            IconAction {visible:!groupEdit.visible;glyph:'edit';Accessible.name:'Rename '+groupRow.modelData;enabled:groupRow.modelData!=='All' && !settings.busy;onClicked:settings.rename(groupRow.modelData,null)}
                                            IconAction {glyph:groupEdit.visible?'close':'remove';Accessible.name:groupEdit.visible?'Cancel rename':'Remove '+groupRow.modelData;enabled:groupRow.modelData!=='All' && !settings.busy;onClicked:{if(groupEdit.visible)settings.editingGroup='';else settings.remove(groupRow.modelData);}}
                                        }
                                        Rectangle {anchors.left:parent.left;anchors.right:parent.right;anchors.bottom:parent.bottom;height:1;color:Qt.alpha(theme.border,0.25)}
                                    }
                                }
                                Repeater {
                                    model:settings.tab==='templates'?(settings.stateData.tasks || []).filter(t=>t.kind==='template' && settings.matches(t.title)):[]
                                    ActionButton {required property var modelData;Layout.fillWidth:true;text:modelData.title;enabled:!settings.busy;onClicked:{settings.close();settings.editTemplate(modelData);}}
                                }
                            }
                        }
                        Item {visible:settings.tab==='general';Layout.fillHeight:true}
                        Label {text:settings.errorMessage;visible:text!=='';color:'#eb6f92';wrapMode:Text.Wrap;Layout.fillWidth:true;font.pixelSize:12}
                    }
                }
            }
        }
        Dialog {
            id:nameDialog;objectName:'group-name-dialog';parent:panel;anchors.centerIn:parent;modal:true;width:Math.min(440,panel.width-48)
            title:'New group';standardButtons:Dialog.Save|Dialog.Cancel
            background:Rectangle {color:theme.background;border.color:theme.border;radius:10}
            contentItem:TextField {id:nameInput;objectName:'group-name-input';placeholderText:'Group name';selectByMouse:true;onAccepted:if(text.trim())nameDialog.accept()}
            onAccepted:if(nameInput.text.trim())settings.operation(Object.assign({},settings.pending,{target:nameInput.text.trim()}))
        }
        Dialog {
            id:confirmDialog;objectName:'group-confirm-dialog';parent:panel;anchors.centerIn:parent;modal:true;title:'Confirm group change';width:Math.min(460,panel.width-48)
            standardButtons:Dialog.Ok|Dialog.Cancel;background:Rectangle {color:theme.background;border.color:theme.border;radius:10}
            contentItem:Label {id:confirmText;wrapMode:Text.Wrap;color:theme.text}
            onAccepted:settings.operation(settings.pending)
        }
    }
}
