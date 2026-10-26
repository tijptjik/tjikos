import QtQuick

Item {
    id:art
    required property var controller
    property string requestedSource:''
    property string token:''
    property int sequence:0
    // Decode once per cover; resizing the viewport must not reload either slot.
    // Bound texture memory while retaining detail for large detached panels.
    readonly property size decodeSize:Qt.size(2048,2048)
    property int activeSlot:0
    property string queuedSource:''
    property string pendingSource:''
    property bool transitioning:false
    readonly property var current:activeSlot===0?first:second
    readonly property var incoming:activeSlot===0?second:first
    readonly property bool ready:current.status===Image.Ready || transitioning
    readonly property string displayedSource:current.source.toString()
    readonly property bool loading:!!requestedSource && (!!pendingSource || transitioning)

    function request() {
        token=String(++sequence);
        queuedSource='';pendingSource='';
        if(!transitioning)incoming.source='';
        if(!requestedSource)return;
        if(requestedSource.startsWith('/'))receive(token,'file://'+requestedSource.split('/').map(encodeURIComponent).join('/'));
        else if(requestedSource.startsWith('file:///'))receive(token,requestedSource);
        else controller.send({op:'artwork',token:token,url:requestedSource});
    }
    function receive(replyToken,source) {
        if(replyToken!==token || !source)return;
        queuedSource=source;
        loadNext();
    }
    function loadNext() {
        if(transitioning || !queuedSource)return;
        const source=queuedSource;queuedSource='';
        if(current.source.toString()===source && current.status===Image.Ready)return;
        pendingSource=source;incoming.opacity=0;incoming.source=source;
        loaded(incoming);
    }
    function loaded(image) {
        if(transitioning || image!==incoming || !pendingSource || image.source.toString()!==pendingSource)return;
        if(image.status===Image.Error){pendingSource='';return;}
        if(image.status!==Image.Ready)return;
        pendingSource='';transitioning=true;
        fade.target=image;fade.start();
    }
    onRequestedSourceChanged:request()
    Connections {
        target:art.controller
        function onArtworkReceived(token,source){art.receive(token,source);}
    }
    Image {
        id:first;anchors.fill:parent;asynchronous:true;cache:true
        fillMode:Image.PreserveAspectCrop;smooth:true;mipmap:true
        sourceSize:art.decodeSize
        z:art.activeSlot===0?0:1
        onStatusChanged:art.loaded(first)
    }
    Image {
        id:second;anchors.fill:parent;asynchronous:true;cache:true
        fillMode:Image.PreserveAspectCrop;smooth:true;mipmap:true
        sourceSize:art.decodeSize
        z:art.activeSlot===1?0:1
        onStatusChanged:art.loaded(second)
    }
    NumberAnimation {
        id:fade;property:'opacity';from:0;to:1;duration:220;easing.type:Easing.InOutQuad
        onFinished:{art.activeSlot=1-art.activeSlot;art.transitioning=false;art.incoming.source='';art.loadNext();}
    }
}
