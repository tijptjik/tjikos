import QtQuick

// The grab result must stay alive for its image:// URL to remain available.
Canvas {
    id: effect
    property var snapshot
    property real progress: 0
    property real tileWidth: width - 24
    property real tileHeight: height - 24
    property bool started: false
    readonly property url source: snapshot ? snapshot.url : ''
    signal prepared()
    signal finished()
    objectName: 'deleteDerez'
    z: 100
    function begin() {
        if(!started && source.toString() && isImageLoaded(source)) {
            started=true;prepared();dissolve.start();
        }
    }
    onSourceChanged: {if(source.toString()){loadImage(source);Qt.callLater(begin);}}
    onImageLoaded: begin()
    onProgressChanged: requestPaint()
    NumberAnimation {id:dissolve;target:effect;property:'progress';from:0;to:1;duration:380;onFinished:effect.finished()}
    onPaint: {
        const c=getContext('2d');c.reset();
        if(!source.toString() || !isImageLoaded(source))return;
        c.imageSmoothingEnabled=false;
        const size=8;
        for(let y=0;y<tileHeight;y+=size)for(let x=0;x<tileWidth;x+=size) {
            const noise=((x*73+y*151+19)%257)/257;
            const phase=Math.max(0,Math.min(1,progress*1.5-noise*.35-(1-y/tileHeight)*.15));
            if(phase>=1)continue;
            const w=Math.min(size,tileWidth-x),h=Math.min(size,tileHeight-y);
            const shrink=1-phase*.8;
            const dx=Math.round((noise-.5)*phase*26/2)*2;
            const dy=Math.round(phase*phase*(8+noise*16)/2)*2;
            c.globalAlpha=(1-phase)*(phase>.15 && ((Math.floor(progress*24)+x+y)%5===0)?.35:1);
            // Clip the scaled whole snapshot: its physical pixel size can differ
            // from fractional delegate geometry while another removal is moving.
            c.save();
            c.translate(12+x+dx,12+y+dy);c.scale(shrink,shrink);
            c.beginPath();c.rect(0,0,w,h);c.clip();
            c.drawImage(source,-x,-y,tileWidth,tileHeight);
            c.restore();
        }
    }
}
