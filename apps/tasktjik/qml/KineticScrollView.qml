import QtQuick
import QtQuick.Controls

ScrollView {
    id: root
    signal backgroundClicked()
    property real wheelStep: 160
    property real wheelSpeedLimit: 2800
    property real wheelDeceleration: 8000
    property bool wheelScrolling: false
    property real lastWheelTime: 0
    property int lastWheelDirection: 0
    property real wheelBoost: 1

    function stopWheelScroll() {
        if(wheelScrolling && contentItem)contentItem.cancelFlick();
        wheelScrolling=false;lastWheelTime=0;wheelBoost=1;
    }
    onVisibleChanged:if(!visible)stopWheelScroll()
    onEnabledChanged:if(!enabled)stopWheelScroll()

    // Flickable consumes blank viewport presses before an outer background can.
    // Put this behind its content so controls and drag gestures retain priority.
    // Keep it out of the content item so automatic content sizing still works.
    MouseArea {
        parent:root.contentItem
        anchors.fill:parent
        onClicked:root.backgroundClicked()
    }
    Binding {target:root.contentItem.contentItem;property:'z';value:1}

    function scrollWheel(event) {
        const flick=contentItem;
        // Pixel deltas and phased gestures already carry touchpad momentum.
        // Qt supplies deltas with the user's natural-scroll preference applied.
        if(event.pixelDelta.x || event.pixelDelta.y || event.phase!==Qt.NoScrollPhase || event.modifiers!==Qt.NoModifier
                || !event.angleDelta.y || Math.abs(event.angleDelta.x)>Math.abs(event.angleDelta.y)) {
            stopWheelScroll();event.accepted=false;return;
        }
        if(!flick || flick.contentHeight<=flick.height){event.accepted=false;return;}
        const direction=event.angleDelta.y<0?1:-1;
        const ticks=Math.abs(event.angleDelta.y)/120;
        const now=Date.now();
        const elapsed=now-lastWheelTime;
        const continuing=direction===lastWheelDirection && elapsed<220;
        const cadence=continuing?Math.max(0,1-elapsed/220):0;
        wheelBoost=Math.min(2,continuing?wheelBoost*(1+0.35*cadence):1);
        const distance=wheelStep*ticks*wheelBoost;
        const speed=continuing && wheelScrolling?Math.max(0,direction*flick.verticalVelocity):0;
        // Animate the entire distance: moving contentY here makes every notch
        // visibly jump before the glide. Carry existing momentum into the flick.
        const remaining=distance+speed*speed/(2*wheelDeceleration);
        const low=flick.originY,high=low+flick.contentHeight-flick.height;
        flick.cancelFlick();
        if(flick.contentY<low || flick.contentY>high)
            flick.contentY=Math.max(low,Math.min(high,flick.contentY));
        const available=direction>0?high-flick.contentY:flick.contentY-low;
        event.accepted=true;
        // Consume outward wheel input at an edge without restarting a flick.
        if(available<0.5){stopWheelScroll();return;}
        const velocity=Math.min(wheelSpeedLimit,Math.sqrt(2*wheelDeceleration*Math.min(remaining,available)));
        lastWheelTime=now;lastWheelDirection=direction;wheelScrolling=true;
        flick.flick(0,-direction*velocity);
    }

    // Apply these only to wheel glides, retaining Qt's touchpad settings.
    Binding {
        target:root.contentItem;property:'flickDeceleration'
        value:root.wheelDeceleration;when:root.wheelScrolling
        restoreMode:Binding.RestoreBindingOrValue
    }
    Binding {
        target:root.contentItem;property:'maximumFlickVelocity'
        value:root.wheelSpeedLimit;when:root.wheelScrolling
        restoreMode:Binding.RestoreBindingOrValue
    }
    Binding {
        target:root.contentItem;property:'boundsBehavior'
        value:Flickable.StopAtBounds;when:root.wheelScrolling
        restoreMode:Binding.RestoreBindingOrValue
    }
    Binding {
        target:root.contentItem;property:'boundsMovement'
        value:Flickable.StopAtBounds;when:root.wheelScrolling
        restoreMode:Binding.RestoreBindingOrValue
    }
    MouseArea {
        objectName:'wheel-overlay'
        parent:root
        anchors.fill:parent
        z:1
        acceptedButtons:Qt.AllButtons
        scrollGestureEnabled:false
        onWheel:event=>root.scrollWheel(event)
        onPressed:mouse=>{root.stopWheelScroll();mouse.accepted=false;}
    }
    Connections {
        target:root.contentItem
        function onMovementEnded(){root.wheelScrolling=false;}
        function onDraggingChanged(){if(root.contentItem.dragging)root.stopWheelScroll();}
    }
    Connections {
        target:root.ScrollBar.vertical
        function onPressedChanged(){if(target.pressed)root.stopWheelScroll();}
    }
    Keys.onPressed:event=>{stopWheelScroll();event.accepted=false;}
}
