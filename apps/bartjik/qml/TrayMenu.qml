import QtQuick
import QtQuick.Controls
import Quickshell

PopupWindow {
    id:popup
    property var handles: []
    property var theme: ({surface:'#26233a',text:'#e5e5e5',border:'#555169',accent:'#ff53a6',font:'sans-serif',fontSize:14})
    property bool opened: false
    readonly property var entries: opener.children.values
    visible: opened && handles.length>0 && !!handles[0]
    implicitWidth: 260
    implicitHeight: Math.min((anchor.window?.screen?.height || 720)-40,menuColumn.implicitHeight+16)
    color:'transparent'
    // Non-grabbing popup: pointer travel and IPC/keyboard opening need no input serial.
    grabFocus: false
    anchor.edges: Edges.Bottom | Edges.Left
    anchor.gravity: Edges.Bottom | Edges.Right
    anchor.adjustment: PopupAdjustment.Flip | PopupAdjustment.Slide
    function openMenu(handle,window,rect,style) {
        anchor.window=window;anchor.rect.x=rect.x;anchor.rect.y=rect.y;
        anchor.rect.width=rect.width;anchor.rect.height=rect.height;
        if(style)theme=style;
        handles=[handle];opened=true;
    }
    function closeMenu() {opened=false;handles=[];}
    function activate(index) {
        const e=entries[index];if(!e || !e.enabled || e.isSeparator)return;
        if(e.hasChildren)handles=handles.concat([e]);
        else {e.triggered();closeMenu();}
    }
    QsMenuOpener {id:opener;menu:popup.handles.length?popup.handles[popup.handles.length-1]:null}
    component MenuButton: Button {
        id:control
        hoverEnabled:true
        padding:10
        verticalPadding:6
        background:Rectangle {
            radius:6
            color:control.down?popup.theme.border:(control.hovered?popup.theme.border:'transparent')
            border.width:control.activeFocus?1:0
            border.color:popup.theme.accent
        }
        contentItem:Text {
            text:control.text
            textFormat:Text.PlainText
            color:control.enabled?popup.theme.text:(popup.theme.muted || '#a5a0ba')
            font.family:popup.theme.font
            font.pixelSize:popup.theme.fontSize
            elide:Text.ElideRight
            verticalAlignment:Text.AlignVCenter
        }
    }
    Rectangle {
        anchors.fill:parent;color:popup.theme.surface;radius:10;border.color:popup.theme.border
        ScrollView {
            anchors.fill:parent;anchors.margins:8;clip:true
            contentWidth:availableWidth
            Column {
                id:menuColumn;width:parent.width;spacing:3
                Row {
                    width:parent.width;height:32;spacing:4
                    MenuButton {height:parent.height;text:'‹ Back';visible:popup.handles.length>1;onClicked:popup.handles=popup.handles.slice(0,-1)}
                    MenuButton {height:parent.height;text:'Close';onClicked:popup.closeMenu()}
                }
                Repeater {
                    model:popup.entries
                    delegate: Item {
                        required property var modelData
                        required property int index
                        width:menuColumn.width;height:modelData.isSeparator?8:34
                        Rectangle {visible:modelData.isSeparator;anchors.centerIn:parent;width:parent.width;height:1;color:popup.theme.border}
                        MenuButton {
                            anchors.fill:parent;visible:!modelData.isSeparator;enabled:modelData.enabled
                            onClicked:popup.activate(index)
                            text:(modelData.checkState===Qt.Checked?'✓ ':'')+modelData.text+(modelData.hasChildren?' ›':'')
                        }
                    }
                }
            }
        }
    }
}
