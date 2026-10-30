import QtQuick
import "BatteryLogic.js" as Battery
import "MorphLogic.js" as Morph

Canvas {
    id:root
    objectName:'battery-indicator'
    required property var battery
    required property var theme
    property bool animate:true
    readonly property int count:Battery.pips(battery.percent)
    readonly property string tone:Battery.tone(count,battery.percent)
    readonly property color chargeColor:theme['battery'+tone[0].toUpperCase()+tone.slice(1)] || ({green:'#9ccf7a',yellow:'#f6c177',red:'#eb6f92'})[tone]
    readonly property color neutral:theme.text || '#e5e5e5'
    readonly property color empty:theme.muted || '#a5a0ba'
    property real progress:battery.external?1:0
    readonly property bool morphing:motion.running
    Behavior on progress {enabled:root.animate;NumberAnimation {id:motion;duration:220;easing.type:Easing.InOutCubic}}
    Accessible.role:Accessible.Indicator
    Accessible.name:(battery.external?'AC power':'Battery')+', '+Math.round(battery.percent)+' percent'
    renderTarget:Canvas.Image
    onProgressChanged:requestPaint()
    onCountChanged:requestPaint()
    onChargeColorChanged:requestPaint()
    onNeutralChanged:requestPaint()
    onEmptyChanged:requestPaint()
    onWidthChanged:requestPaint()
    onHeightChanged:requestPaint()
    onPaint:{
        const c=getContext('2d');c.reset();c.scale(width/24,height/24);
        c.lineWidth=1.5;c.lineCap='round';c.lineJoin='round';
        Morph.interpolate(Battery.grid,Battery.flash,progress).forEach((path,i)=>{
            const lit=i<count, ink=lit?chargeColor:empty, t=progress;
            const color=Qt.rgba(ink.r+(neutral.r-ink.r)*t,ink.g+(neutral.g-ink.g)*t,ink.b+(neutral.b-ink.b)*t,1);
            c.fillStyle=color;c.strokeStyle=color;
            c.beginPath();path.points.forEach((p,j)=>{if(j)c.lineTo(p[0],p[1]);else c.moveTo(p[0],p[1]);});c.closePath();
            const opacity=path.opacity*((lit?1:.25)*(1-t)+t);
            if(path.fill>0){c.globalAlpha=opacity*path.fill;c.fill();}
            if(path.stroke>0){c.globalAlpha=opacity*path.stroke;c.stroke();}
        });
    }
}
