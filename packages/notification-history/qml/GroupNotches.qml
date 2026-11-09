import QtQuick

Item {
    id: notches
    property int count: 1
    property int animationMs: 220
    property real notchWidth: 3
    property real notchSpacing: 10
    function notchY(index) {
        return height / 2 + (index - (cappedCount - 1) / 2) * notchSpacing - 3;
    }
    property color color: "#aaaaaa"
    property real inkOpacity: 0.75
    readonly property int cappedCount: Math.max(1, Math.min(5, count))
    readonly property bool grouped: count > 1
    property bool ready: false
    property int revision: 0
    readonly property bool morphing: {
        const changed = revision;
        for (let i = 0; i < slices.count; ++i) {
            const piece = slices.itemAt(i);
            if (piece && piece.moving) return true;
        }
        return false;
    }
    opacity: grouped ? inkOpacity : 0
    Component.onCompleted: ready = true
    Behavior on opacity {
        enabled: notches.ready
        NumberAnimation { duration: notches.animationMs; easing.type: Easing.OutCubic }
    }

    // Sixty persistent slices divide evenly into one through five bars.
    // Redistributing them makes real splits/merges, including the centre bar's
    // two halves moving out to join its neighbours on a three-to-two change.
    Repeater {
        id: slices
        model: 60
        onItemAdded: notches.revision++
        onItemRemoved: notches.revision++
        Rectangle {
            id: slice
            required property int index
            readonly property bool moving: slide.running || resize.running
            visible: notches.morphing
            readonly property int perNotch: 60 / notches.cappedCount
            readonly property int band: Math.floor(index / perNotch)
            readonly property int part: index % perNotch
            x: notches.width - width
            width: notches.notchWidth
            height: 6 / perNotch
            y: notches.notchY(band) + part * (6 / perNotch)
            color: notches.color
            antialiasing: false
            Behavior on y {
                enabled: notches.ready
                NumberAnimation { id: slide; duration: notches.animationMs; easing.type: Easing.OutCubic }
            }
            Behavior on height {
                enabled: notches.ready
                NumberAnimation { id: resize; duration: notches.animationMs; easing.type: Easing.OutCubic }
            }
        }
    }
    // Once joined, paint each bar as a single shape with clean rounded ends.
    Repeater {
        model: notches.cappedCount
        Rectangle {
            required property int index
            visible: !notches.morphing
            x: notches.width - width
            y: notches.notchY(index)
            width: notches.notchWidth
            height: 6
            radius: 1
            color: notches.color
        }
    }
}
