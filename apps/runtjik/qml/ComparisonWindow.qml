import QtQuick
import Quickshell

// Dedicated ordinary window: the comparison controller installs workspace rules
// before launch. Production layer surfaces and their outside-click hooks are untouched.
FloatingWindow {
    required property var targetScreen
    property bool pinned: false
    property bool closeOnOutsideClick: true
    property bool ownsKeyboard: true
    property bool acquiringKeyboard: false
    property bool styled: true
    property bool observed: true
    readonly property bool placed: backingWindowVisible
    property string address: ''
    property string placementError: ''
    property string clickWatchError: ''
    property real frameX: 0
    property real frameY: 0
    property real frameWidth: 720
    property real frameHeight: 600
    function acquireKeyboard() { ownsKeyboard=true; }
    function updateBounds(x,y,w,h) { frameX=x;frameY=y;frameWidth=w;frameHeight=h; }
    title: 'Runtjik comparison Quickshell '+Quickshell.env('RUNTJIK_COMPARE_ID')
    screen: targetScreen
    implicitWidth: Number(Quickshell.env('RUNTJIK_COMPARE_WIDTH') || 720)
    implicitHeight: Number(Quickshell.env('RUNTJIK_COMPARE_HEIGHT') || 600)
    color: 'transparent'
}
