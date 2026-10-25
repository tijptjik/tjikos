import QtQuick
import QtQuick.Layouts

// Collapse layout space as well as ink; retain the controls while sliding.
Item {
    id: section
    property bool expanded: false
    property real reveal: expanded ? 1 : 0
    default property alias contents: body.data
    implicitHeight: body.implicitHeight * reveal
    visible: reveal > 0
    enabled: expanded
    clip: true
    opacity: reveal
    Behavior on reveal { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
    ColumnLayout {
        id: body
        width: section.width
        y: -(1 - section.reveal) * 8
        spacing: 8
    }
}
