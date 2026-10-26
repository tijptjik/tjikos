import QtQuick
import "MorphData.js" as Data
import "MorphLogic.js" as Morph

Canvas {
    id:root
    objectName:"toggleMorph"
    required property string symbol
    property bool animate:true
    property string previous:''
    property var sourceShape:[]
    property var targetShape:[]
    property real progress:1
    property bool ready:false
    property color ink:'#ffffff'
    readonly property bool morphing:motion.running
    visible:morphing
    renderTarget:Canvas.Image
    onSymbolChanged: {
        if(ready && animate && Morph.compatible(previous,symbol)) {
            sourceShape=motion.running?Morph.interpolate(sourceShape,targetShape,progress):Data.shapes[previous];
            targetShape=Data.shapes[symbol];motion.stop();progress=0;motion.start();
        } else {motion.stop();progress=1;}
        previous=symbol;
    }
    Component.onCompleted:{previous=symbol;ready=true;}
    onProgressChanged:requestPaint()
    onWidthChanged:requestPaint()
    NumberAnimation {id:motion;target:root;property:'progress';from:0;to:1;duration:220;easing.type:Easing.InOutCubic}
    onPaint:{
        const c=getContext('2d');c.reset();c.scale(width/24,height/24);
        c.strokeStyle=ink;c.fillStyle=ink;c.lineWidth=1.5;c.lineCap='round';c.lineJoin='round';
        Morph.interpolate(sourceShape,targetShape,progress).forEach(path=>{
            if(!path.points.length)return;
            c.beginPath();path.points.forEach((p,i)=>{if(i)c.lineTo(p[0],p[1]);else c.moveTo(p[0],p[1]);});
            if(path.closed)c.closePath();
            if(path.fill>0){c.globalAlpha=path.opacity*path.fill;c.fill();}
            if(path.stroke>0){c.globalAlpha=path.opacity*path.stroke;c.stroke();}
        });
    }
}
