import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland

Scope {
    id:confirmation
    required property var controller
    readonly property var change:controller.state.monitorChange || null
    readonly property var theme:controller.bars[0]?.style || {font:'sans-serif',text:'#e5e5e5',muted:'#a5a0ba',accent:'#ff53a6',background:'#1f1d2e',surface:'#26233a',border:'#555169'}
    property double clock:Date.now()/1000
    readonly property int remaining:Math.max(0,Math.ceil((change?.deadline || 0)-clock))
    function decide(decision){if(change)controller.send({op:'monitorDecision',token:change.token,decision:decision});}
    Timer {interval:200;repeat:true;running:!!confirmation.change;onTriggered:confirmation.clock=Date.now()/1000}
    Variants {
        model:confirmation.change?Quickshell.screens:[]
        delegate:PanelWindow {
            required property var modelData
            screen:modelData;visible:!!confirmation.change
            implicitWidth:Math.min(420,screen.width-24);implicitHeight:170
            exclusionMode:ExclusionMode.Ignore;color:'transparent'
            WlrLayershell.namespace:'bartjik-display-confirmation'
            WlrLayershell.layer:WlrLayer.Overlay
            WlrLayershell.keyboardFocus:WlrKeyboardFocus.OnDemand
            Rectangle {
                anchors.fill:parent;radius:12;color:confirmation.theme.background;border.width:1;border.color:confirmation.theme.accent
                ColumnLayout {
                    anchors.fill:parent;anchors.margins:16;spacing:10
                    Text {text:confirmation.change?.phase==='confirm'?'Keep these display settings?':'Restoring / applying displays…';font.family:confirmation.theme.font;font.pixelSize:15;color:confirmation.theme.text;Layout.fillWidth:true;wrapMode:Text.Wrap}
                    Text {text:confirmation.change?.error || 'Previous layout returns in '+confirmation.remaining+' seconds.';font.family:confirmation.theme.font;font.pixelSize:12;color:confirmation.theme.muted;Layout.fillWidth:true;wrapMode:Text.Wrap}
                    RowLayout {
                        Layout.alignment:Qt.AlignHCenter;spacing:12
                        PanelButton {objectName:'display-revert';theme:confirmation.theme;showLabel:true;text:'Revert';onClicked:confirmation.decide('revert')}
                        PanelButton {objectName:'display-keep';theme:confirmation.theme;showLabel:true;text:'Keep changes';enabled:confirmation.change?.phase==='confirm' && confirmation.remaining>0;onClicked:confirmation.decide('keep')}
                    }
                }
            }
        }
    }
}
