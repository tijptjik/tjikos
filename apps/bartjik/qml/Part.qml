import QtQuick
import QtQuick.Controls
import QtQuick.Effects
import Quickshell
import "Logic.js" as Logic
import "IconRegistry.js" as Icons

Item {
    id: part
    required property var row
    required property string kind
    required property var moduleConfig
    required property var theme
    required property var context
    required property var controller
    required property var window
    property bool alternate: false
    property bool hoverEntered: false
    readonly property bool hovered: hover.hovered
    readonly property bool centeredWorkspaceLabel: kind==='text' && moduleConfig.provider==='workspaces' && moduleConfig.appearance==='ring'
    readonly property bool activeWorkspaceDot: centeredWorkspaceLabel && (row.state?.active===true || row.state?.focused===true)
    property real workspaceActivation: activeWorkspaceDot ? 1 : 0
    Behavior on workspaceActivation {NumberAnimation {duration:200;easing.type:Easing.InOutCubic}}
    readonly property bool workspacePreviewEnabled: moduleConfig.provider==='workspaces' && moduleConfig.preview?.enabled===true
    readonly property bool cryptoHoverEnabled: moduleConfig.provider==='crypto' && moduleConfig.panel?.hover===true
    function previewContext() {return Object.assign({},context,{item:row.id,part:kind,anchor:anchor()});}
    readonly property var value: row[kind] || {}
    readonly property bool batteryIcon: kind==='icon' && moduleConfig.provider==='battery' && !!row.battery
    readonly property var style: Logic.partStyle(theme,moduleConfig,row,kind,hover.hovered)
    readonly property string proIcon: kind==='icon' && value.kind==='symbol' ? Icons.resolve(String(value.value || ''),style.iconSet) : ''
    readonly property bool trayUnread: kind==='icon' && moduleConfig.provider==='icontray' && row.state?.urgent===true
    readonly property bool glyph: kind!=='icon' || value.kind==='glyph' || (value.kind==='symbol' && !proIcon)
    readonly property string label: kind!=='icon' && alternate && row.alternate ? row.alternate : String(value.value || '')
    implicitWidth: Math.max(style.minItemSize,kind==='icon'?style.iconSize:Math.min(naturalTextWidth,style.maxTextWidth))
    implicitHeight: Math.max(style.iconSize,style.fontSize*1.3,style.minItemSize)
    opacity: row.enabled===false ? .5 : 1
    function anchor() {
        const pos=mapToItem(window.contentItem,0,0);
        return {x:pos.x,y:pos.y,width:width,height:height};
    }
    function fire(event) {
        const action=Logic.choose(moduleConfig,row,kind,event);
        if(!action || row.enabled===false)return;
        if(event==='hoverEnter' && ['audio.panel','network.panel'].includes(action))return; // Shared island hover owns the dwell timer.
        if(action==='datetime.toggle') {alternate=!alternate;return;}
        controller.act(action,Object.assign({},context,{item:row.id,part:kind,event:event,anchor:anchor()}),window,row);
    }
    TextMetrics {id:measurement;font:labelText.font;text:part.label}
    readonly property real naturalTextWidth: Math.ceil(measurement.advanceWidth)
    property real scrollOffset: 0
    property bool tickerReady: false
    function resetTicker() {tickerReady=false;scrollOffset=0;settleText.restart();}
    Timer {id:settleText;interval:0;onTriggered:part.tickerReady=true}
    readonly property bool ticking: kind!=='icon' && style.marquee===true && naturalTextWidth>width+1
    Item {
        anchors.centerIn:parent;width:parent.width;height:parent.height;clip:true
        opacity:1-part.workspaceActivation
        scale:1-.65*part.workspaceActivation
        Text {
            id:labelText
            objectName: "part-label"
            anchors.verticalCenter:parent.verticalCenter
            // Center the visible glyph, rather than the font's advance box and line height.
            anchors.verticalCenterOffset: part.centeredWorkspaceLabel ? height/2-baselineOffset-measurement.tightBoundingRect.y-measurement.tightBoundingRect.height/2 : 0
            x:part.centeredWorkspaceLabel ? (parent.width-measurement.tightBoundingRect.width)/2-measurement.tightBoundingRect.x : part.ticking?part.scrollOffset:(parent.width-width)/2
            width:part.kind==='icon'?part.style.iconSize:part.ticking?part.naturalTextWidth:Math.min(part.naturalTextWidth,parent.width)
            text:part.label;textFormat:Text.PlainText
            visible:part.glyph && !morph.morphing && !part.batteryIcon
            elide:part.kind==='icon' || part.style.marquee===true?Text.ElideNone:Text.ElideRight
            fontSizeMode:part.kind==='icon'?Text.HorizontalFit:Text.FixedSize
            minimumPixelSize:8
            color:part.activeWorkspaceDot || part.workspaceActivation>0 ? '#ffffff' : part.style.color || part.style.text
            font.family:part.kind==='icon' && part.value.kind==='symbol'?part.style.iconFont:part.style.font
            font.pixelSize:part.kind==='icon'?part.style.iconSize:part.style.fontSize
        }
        SequentialAnimation {
            id:ticker;running:part.tickerReady && part.ticking && part.visible && part.window.visible;loops:Animation.Infinite
            PauseAnimation {duration:1200}
            NumberAnimation {target:part;property:'scrollOffset';from:0;to:Math.min(0,part.width-part.naturalTextWidth);duration:Math.max(800,(part.naturalTextWidth-part.width)/35*1000);easing.type:Easing.Linear}
            PauseAnimation {duration:1400}
            NumberAnimation {target:part;property:'scrollOffset';to:0;duration:300;easing.type:Easing.InOutCubic}
        }
    }
    Rectangle {
        objectName:'workspace-active-dot'
        anchors.centerIn:parent;width:6;height:6;radius:3
        visible:part.centeredWorkspaceLabel && part.workspaceActivation>0
        color:'#ffffff';opacity:part.workspaceActivation
        scale:.35+.65*part.workspaceActivation
    }
    onLabelChanged: resetTicker()
    onNaturalTextWidthChanged: resetTicker()
    onWidthChanged: resetTicker()
    Component.onCompleted: resetTicker()

    Image {
        id: artwork
        anchors.centerIn: parent; width: part.style.iconSize; height: width
        visible: !part.glyph && !morph.morphing && !part.batteryIcon
        source: {
            // Read kind and value together: live icon overrides change both at once.
            const icon=part.value, label=String(icon.value || '');
            if(part.kind!=='icon' || icon.kind==='glyph') return '';
            if(icon.kind==='symbol') {
                const match=Icons.resolve(label,part.style.iconSet);
                return match ? Qt.resolvedUrl(match) : '';
            }
            if(icon.kind==='theme') return Quickshell.iconPath(label,true);
            return label.startsWith('/') ? 'file://'+label : label;
        }
        fillMode: Image.PreserveAspectFit
        asynchronous: part.value.kind!=='native'
        smooth: true
        mipmap: true
        sourceSize: Qt.size(Math.ceil(part.style.iconSize*Math.max(2,part.window?.screen?.devicePixelRatio || 1)),Math.ceil(part.style.iconSize*Math.max(2,part.window?.screen?.devicePixelRatio || 1)))
        layer.enabled: part.style.greyscaleIcons===true || part.style.monochromeIcon===true
        layer.effect: MultiEffect {saturation:-1;brightness:part.style.monochromeIcon===true?1:0}
    }
    Rectangle {
        visible: part.trayUnread
        width: 7; height: 7; radius: 3.5
        anchors.right: artwork.right; anchors.top: artwork.top
        anchors.rightMargin: -2; anchors.topMargin: -2
        color: part.style.accent || '#ff53a6'
        border.width: 1; border.color: part.style.background || 'transparent'
    }
    MorphIcon {
        id:morph;anchors.centerIn:parent;width:part.style.iconSize;height:width
        symbol:part.kind==='icon' && part.value.kind==='symbol'?part.label:''
        animate:part.style.animateIcons!==false && part.style.iconSet!=='material'
        ink:part.style.color || part.style.text
    }
    Loader {
        anchors.centerIn:parent;width:part.style.iconSize;height:width
        active:part.batteryIcon
        sourceComponent:BatteryIcon {
            battery:part.row.battery;theme:part.style
            animate:part.style.animateIcons!==false
        }
    }
    Text {
        anchors.centerIn: parent
        visible: !part.glyph && !morph.morphing && !part.batteryIcon && (artwork.status===Image.Error || artwork.status===Image.Null)
        text: '◇';textFormat: Text.PlainText
        color: part.style.color || part.style.text
        font.family: part.style.font;font.pixelSize: part.style.iconSize
    }
    Timer {id: single;interval: part.moduleConfig.doubleClickInterval || Qt.styleHints.mouseDoubleClickInterval;onTriggered: part.fire('leftClick')}
    Timer {id: enter;interval: part.moduleConfig.hoverDelay===undefined?250:part.moduleConfig.hoverDelay;onTriggered: {part.hoverEntered=true;part.fire('hoverEnter');}}
    HoverHandler {
        id:hover
        blocking:false
        onHoveredChanged: {
            if(hovered){enter.restart();}
            else {enter.stop();if(part.hoverEntered)part.fire('hoverLeave');part.hoverEntered=false;}
        }
    }
    MouseArea {
        id: mouse
        anchors.fill: parent
        enabled: part.row.enabled!==false
        hoverEnabled: false
        acceptedButtons: Qt.LeftButton | Qt.RightButton | Qt.MiddleButton
        onClicked: event => {
            const name=event.button===Qt.LeftButton?'leftClick':event.button===Qt.RightButton?'rightClick':'middleClick';
            if(name==='leftClick' && Logic.choose(part.moduleConfig,part.row,part.kind,'doubleClick'))single.restart();
            else part.fire(name);
        }
        onDoubleClicked: event => {
            if(event.button===Qt.LeftButton && Logic.choose(part.moduleConfig,part.row,part.kind,'doubleClick')) {
                single.stop();part.fire('doubleClick');event.accepted=true;
            } else event.accepted=false;
        }
        onWheel: event => {part.fire(event.angleDelta.y>0?'scrollUp':'scrollDown');event.accepted=true;}
    }
    PanelToolTip {
        id: tooltip
        objectName: "part-tooltip"
        theme:part.theme
        visible: hover.hovered && part.hoverEntered && !!part.row.tooltip && part.moduleConfig.provider!=='crypto' && !part.workspacePreviewEnabled && !part.moduleConfig.panel?.hover && !['audio.panel','network.panel'].includes(Logic.choose(part.moduleConfig,part.row,part.kind,'hoverEnter'))
        text: String(part.row.tooltip || '')
        // Qt Quick Controls anchors this popup to this part on the same window/screen.
        delay: 0
    }
}
