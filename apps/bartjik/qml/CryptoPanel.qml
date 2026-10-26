import QtQuick
import QtQuick.Controls
import Quickshell
import "Logic.js" as Logic

DetachablePopup {
    id: popup
    stableSurface:true
    nativeDetach:true
    dragFromBody:true
    detachedTitle:pair
    panelShell:controller.panelShell
    panelBody:body
    panelOpen:opened
    onDetachedChanged:if(detached){pinned=true;hoverKey="";pending=null;delay.stop();hide.stop();}
    onDismissed:closePanel()
    required property var controller
    property var pending: null
    property var selectedContext: null
    property var options: ({})
    property string hoverKey: ''
    property string selectionKey: ''
    property bool pinned: false
    readonly property bool waiting: delay.running
    function key(context) {return context.barKey+'/'+context.module+'/'+context.item;}
    function partKey(context) {return key(context)+'/'+(context.part || '');}
    function enter(context,window,row,settings) {
        if(detached || dragging)return;
        const same=hoverKey===partKey(context);
        hide.stop();options=settings || {};hoverKey=partKey(context);
        pending={context:Object.assign({},context,{event:'hoverEnter'}),window:window,row:row};
        if(opened && anchor.window===window){
            panelShell.show(popup);
            if(selectionKey===key(context)){delay.stop();selectedContext=context;position(context,window);}
            else controller.act('crypto.panel',pending.context,window,row);
            return;
        }
        if(panelShell.canCross(window)){delay.stop();controller.act('crypto.panel',pending.context,window,row);}
        else if(!same || !delay.running)delay.restart();
    }
    function leave(context) {
        if(partKey(context)!==hoverKey)return;
        hoverKey='';pending=null;delay.stop();if(!pinned)hide.restart();
    }
    function pointerHeld() {
        const pos=attachedPosition(),side=anchor.window?.bar.side,w=body.width,h=body.height;
        return Logic.panelHeld(Date.now()-controller.pointerUpdated<1500?controller.pointer:null,panelScreen?.name,selectedContext?.hoverRegion,
            {x:pos.x+(side==='r'?w:0),y:pos.y+(side==='b'?h:0)},w,h,side);
    }
    function closePanel() {opened=false;resetDetach();delay.stop();hide.stop();hoverKey='';pending=null;pinned=false;}
    function position(context,window) {
        if(detached || dragging)return;
        anchor.window=window;
        const point=Logic.panelAnchor(context.anchor,window.geometry,window.bar.side,window.screen.width,window.screen.height,implicitWidth,implicitHeight);
        anchor.rect.x=point.x;anchor.rect.y=point.y;anchor.rect.width=0;anchor.rect.height=0;
    }
    Timer {id:delay;interval:popup.options.delay ?? 300;onTriggered:if(popup.pending?.window?.bar && popup.hoverKey)popup.controller.act('crypto.panel',popup.pending.context,popup.pending.window,popup.pending.row)}
    Timer {id:hide;repeat:true;interval:Logic.panelCloseDelay(popup.options);onTriggered:if(!popup.panelShell.holding(popup) && !inside.hovered && !popup.hoverKey && !popup.pinned && !popup.dragging && !popup.pointerHeld())popup.closePanel()}
    Connections {
        target:popup.controller
        function onBarsChanged(){if(popup.selectedContext && !popup.controller.bars.some(b=>b.key===popup.selectedContext.barKey))popup.closePanel();}
        function onVisibilityChanged(){if(!popup.detached && !popup.dragging && popup.anchor.window?.bar && !popup.controller.shown(popup.anchor.window.bar))popup.closePanel();}
    }
    property bool opened: false
    property string token: ''
    property string pair: ''
    property string price: ''
    property var headerIcon: ({})
    property var changes: ({})
    property var chart: ({})
    property var cachedCharts: ({})
    property var requests: ({})
    property string cacheKey: ''
    function receive(requestToken,result) {
        const cache=requests[requestToken];
        if(!cache)return;
        cachedCharts[cache]={chart:result,requestedAt:cachedCharts[cache]?.requestedAt || Date.now()};
        if(!result.loading)delete requests[requestToken];
        if(cacheKey===cache)chart=result;
    }
    property var theme: ({background:'#1f1d2e',border:'#555169',surface:'#26233a',text:'#e5e5e5',muted:'#a5a0ba',accent:'#ff53a6',font:'sans-serif',fontSize:14})
    readonly property var points: chart.points || []
    readonly property real low: points.length?Math.min(...points.map(p=>p[1])):0
    readonly property real high: points.length?Math.max(...points.map(p=>p[1])):0
    visible:false
    implicitWidth: Math.min(440,(anchor.window?.screen?.width || 480)-24)
    implicitHeight: Logic.previewHeight(theme,anchor.window?.screen?.height)
    color: 'transparent'
    grabFocus: false
    anchor.edges: Edges.Top | Edges.Left
    anchor.gravity: anchor.window?.bar.side==='t'?Edges.Bottom|Edges.Right:anchor.window?.bar.side==='l'?Edges.Bottom|Edges.Right:anchor.window?.bar.side==='r'?Edges.Bottom|Edges.Left:Edges.Top|Edges.Right
    anchor.adjustment: PopupAdjustment.Flip | PopupAdjustment.Slide
    function open(context,window,row) {
        if(detached || dragging)return false;
        delay.stop();hide.stop();selectedContext=context;
        pinned=context.event!=='hoverEnter';position(context,window);theme=window.bar.style;
        if(opened && selectionKey===key(context))return false;
        selectionKey=key(context);
        theme=window.bar.style;
        token=context.barKey+'/'+context.module+'/'+row.id+'/'+(++controller.cryptoRequestSerial);
        pair=row.id;price=row.text?.value || '';headerIcon=row.icon || {};changes=row.changes || {};
        cacheKey=JSON.stringify([window.bar.sources[context.module],row.id]);
        const cached=cachedCharts[cacheKey];
        chart=cached?.chart || {loading:true};opened=true;
        if(cached && Date.now()-cached.requestedAt<60000)return false;
        if(Object.keys(cachedCharts).length>=64)delete cachedCharts[Object.keys(cachedCharts)[0]];
        cachedCharts[cacheKey]={chart:chart,requestedAt:Date.now()};requests[token]=cacheKey;
        chart=Object.assign({},chart,{loading:true});
        return true;
    }
    function delta(key) {const v=changes[key];return typeof v==='number' && isFinite(v)?(v>0?'+':'')+v.toFixed(2)+'%':'—';}
    function formatted(value) {return value.toLocaleString(Qt.locale('en_US'),'f',value>=100?0:2);}
    function capture(path) {body.grabToImage(result=>result.saveToFile(path));}
    onChartChanged: graph.requestPaint()
    Rectangle {
        id:body;parent:popup.bodyParent;opacity:popup.bodyOpacity;anchors.fill:parent;color:popup.theme.background;radius:12;border.width:1;border.color:popup.theme.panelBorder || popup.theme.border
        HoverHandler {id:inside;onHoveredChanged:{if(hovered)hide.stop();else if(!popup.hoverKey && !popup.pinned)hide.restart();}}
        Column {
            anchors.fill:parent;anchors.margins:16;spacing:10
            Row {
                width:parent.width;height:28
                Item {
                    width:30;height:28
                    Image {anchors.verticalCenter:parent.verticalCenter;width:22;height:22;fillMode:Image.PreserveAspectFit;sourceSize:Qt.size(44,44);visible:popup.headerIcon.kind==='image';source:visible?popup.headerIcon.value:''}
                    Text {anchors.verticalCenter:parent.verticalCenter;visible:popup.headerIcon.kind!=='image';text:popup.headerIcon.value || '';textFormat:Text.PlainText;color:popup.theme.text;font.pixelSize:20;font.family:popup.headerIcon.kind==='symbol'?popup.theme.iconFont:popup.theme.font}
                }
                Text {width:parent.width-30-closeButton.width;text:popup.pair+'  ·  '+popup.price;textFormat:Text.PlainText;color:popup.theme.text;font.family:popup.theme.font;font.pixelSize:17;anchors.verticalCenter:parent.verticalCenter}
                PanelHandle {id:closeButton;visible:popup.detached;width:visible?32:0;height:28;popup:popup;theme:popup.theme}
            }
            Row {
                spacing:24
                Repeater {
                    model:[{key:'1h',label:'1 hour'},{key:'24h',label:'24 hours'},{key:'7d',label:'1 week'}]
                    Column {
                        required property var modelData
                        spacing:4
                        Text {text:modelData.label;color:popup.theme.muted;font.pixelSize:11}
                        Text {text:popup.delta(modelData.key);color:popup.theme.text;font.pixelSize:16;font.family:popup.theme.font}
                    }
                }
            }
            Item {
                width:parent.width;height:Math.max(0,body.height-164)
                Item {
                    anchors.fill:parent;anchors.rightMargin:58
                    opacity:popup.chart.loading ? .18 : .2
                    SequentialAnimation on opacity {
                        running:popup.panelVisible && popup.chart.loading===true;loops:Animation.Infinite
                        NumberAnimation {from:.12;to:.4;duration:700;easing.type:Easing.InOutSine}
                        NumberAnimation {from:.4;to:.12;duration:700;easing.type:Easing.InOutSine}
                    }
                    Repeater {
                        model:3
                        Rectangle {required property int index;x:0;y:8+(parent.height-16)*index/2;width:parent.width;height:1;color:popup.theme.muted}
                    }
                }
                Canvas {
                    id:graph;anchors.fill:parent;anchors.rightMargin:58
                    onWidthChanged:requestPaint()
                    onHeightChanged:requestPaint()
                    onPaint:{
                        const c=getContext('2d');c.reset();
                        if(popup.points.length<2)return;
                        const span=Math.max(popup.high-popup.low,popup.high*.00001,0.00001);
                        const start=popup.points[0][0],duration=Math.max(1,popup.points[popup.points.length-1][0]-start);
                        c.globalAlpha=1;c.strokeStyle=popup.theme.accent;c.lineWidth=2;c.beginPath();
                        popup.points.forEach((p,i)=>{const x=(p[0]-start)/duration*width;const y=height-8-(p[1]-popup.low)/span*(height-16);if(i)c.lineTo(x,y);else c.moveTo(x,y);});c.stroke();
                    }
                }
                Text {anchors.right:parent.right;anchors.top:parent.top;text:popup.points.length?popup.formatted(popup.high):'';color:popup.theme.muted;font.pixelSize:10}
                Text {anchors.right:parent.right;anchors.bottom:parent.bottom;text:popup.points.length?popup.formatted(popup.low):'';color:popup.theme.muted;font.pixelSize:10}
                Text {anchors.centerIn:parent;visible:popup.points.length<2 && !popup.chart.loading;text:'History unavailable';color:popup.theme.muted;font.pixelSize:13}
            }
            Row {
                width:parent.width
                Text {width:parent.width/2;text:'24 hours ago';color:popup.theme.muted;font.pixelSize:10}
                Text {width:parent.width/2;horizontalAlignment:Text.AlignRight;text:'Latest sample';color:popup.theme.muted;font.pixelSize:10}
            }
            Text {
                width:parent.width;elide:Text.ElideRight
                text:popup.chart.error ? (popup.chart.stale?'Cached · ':'')+popup.chart.error : popup.chart.source ? popup.chart.source+' · '+new Date(popup.chart.updated*1000).toLocaleTimeString(Qt.locale(),'HH:mm')+' · changes: quote feed' : ''
                color:popup.theme.muted;font.pixelSize:10;textFormat:Text.PlainText
            }
        }
    }
}
