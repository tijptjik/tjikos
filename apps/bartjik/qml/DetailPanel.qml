import QtQuick
import QtQuick.Controls
import Quickshell
import "Logic.js" as Logic

DetachablePopup {
    id:popup
    detachedTitle:panelTitle
    stableSurface:true
    panelShell:controller.panelShell
    panelBody:body
    panelOpen:opened
    dragFromBody:true
    onDetachedChanged:if(detached){pinned=true;hoverKey="";pending=null;delay.stop();hide.stop();}
    onDismissed:close()
    required property var controller
    property var selected:null
    property var pending:null
    property var options:({})
    property string hoverKey:''
    property bool opened:false
    property bool pinned:false
    property int transitionSerial:0
    property var transitionImage:null
    property real transitionWidth:0
    property real transitionHeight:0
    property real contentProgress:1
    readonly property var bar:selected?.window?.bar
    readonly property var theme:bar?.style || {background:'#1f1d2e',border:'#555169',surface:'#26233a',text:'#e5e5e5',muted:'#a5a0ba',accent:'#ff53a6',font:'sans-serif'}
    readonly property var row:selected && bar ? (controller.items(bar,selected.context.module).find(i=>i.id===selected.context.item) || controller.items(bar,selected.context.module)[0]) : null
    readonly property var panelData:row?.panel || {title:bar?.moduleConfigs[selected?.context.module]?.provider || 'Status',subtitle:'Service unavailable',rows:[{label:'Status',value:'Unavailable',detail:row?.tooltip || 'Waiting for provider'}]}
    readonly property var fields:panelData.rows || []
    readonly property string panelTitle:codexPanel?'Codex':audioPanel?'Audio':networkPanel?'Networking':panelData.title || ''
    readonly property string provider:bar?.moduleConfigs[selected?.context.module]?.provider || ''
    readonly property var feedback:controller.state.panelFeedback && selected && controller.state.panelFeedback.barKey===selected.context.barKey && controller.state.panelFeedback?.module===selected?.context.module ? controller.state.panelFeedback : ({})
    readonly property string tailnetModule:provider==='mullvad'?controller.connectivityPeer(bar,selected?.context.module,'tailscale'):''
    readonly property var tailnetRow:tailnetModule?controller.items(bar,tailnetModule)[0]:null
    function moduleFeedback(module){return controller.state.panelFeedback?.barKey===selected?.context.barKey && controller.state.panelFeedback?.module===module?controller.state.panelFeedback:({});}
    function actionFor(module,name,selection){if(selected)controller.send({op:'panelAction',context:Object.assign({},selected.context,{module:module,action:name,selection:selection})});}
    function action(name,selection){actionFor(selected?.context.module,name,selection);}
    readonly property bool codexPanel:bar?.moduleConfigs[selected?.context.module]?.provider==='codex'
    readonly property bool openrouterPanel:bar?.moduleConfigs[selected?.context.module]?.provider==='openrouter'
    readonly property bool mediaPanel:bar?.moduleConfigs[selected?.context.module]?.provider==='playerctl'
    readonly property bool audioPanel:provider==='audio'
    readonly property bool networkPanel:provider==='network'
    onRowChanged:if(opened && mediaPanel && !row)Qt.callLater(()=>{if(opened && mediaPanel && !row)close();})
    readonly property bool localePanel:bar?.moduleConfigs[selected?.context.module]?.provider==='locale'
    function key(c){return c.barKey+'/'+c.module+'/'+c.item;}
    function locate(context,window){
        if(detached || dragging)return;
        anchor.window=window;
        const p=Logic.panelAnchor(context.anchor,window.geometry,window.bar.side,window.screen.width,window.screen.height,implicitWidth,implicitHeight);
        anchor.rect.x=p.x;anchor.rect.y=p.y;
    }
    function enter(context,window,row,settings){
        if(detached || dragging)return;
        const same=hoverKey===key(context);
        if(opened && selected?.window!==window)close();
        if(selected && key(selected.context)!==key(context))pinned=false;
        hide.stop();hoverKey=key(context);options=settings || {};pending={context:context,window:window};
        if(opened && selected?.window===window){
            panelShell.show(popup);
            if(key(selected.context)===key(context)){++transitionSerial;selected=pending;Qt.callLater(syncGeometry);return;}
            switchCard();return;
        }
        if(panelShell.canCross(window)){delay.stop();reveal();}
        else if(!same || !delay.running)delay.restart();
    }
    function switchCard(){
        const serial=++transitionSerial;
        const oldWidth=contentStack.width,oldHeight=contentStack.height;
        contentStack.grabToImage(image=>{
            if(serial!==transitionSerial || !opened || !pending || detached || dragging)return;
            contentFade.stop();transitionImage=image;transitionWidth=oldWidth;transitionHeight=oldHeight;contentProgress=0;selected=pending;
            Qt.callLater(()=>{
                // Re-entering the same target can invalidate pending grabs, but
                // must still finish a content change already made above.
                if(!opened || detached || dragging)return;
                syncGeometry();contentFade.restart();
            });
        });
    }
    NumberAnimation {
        id:contentFade;target:popup;property:'contentProgress';from:0;to:1;duration:140
        onFinished:popup.transitionImage=null
    }
    function open(context,window,row){
        if(detached || dragging)return;
        enter(context,window,row,window.bar.moduleConfigs[context.module]?.panel);
        delay.stop();if(!opened)reveal();pinned=true;
    }
    function leave(context){if(key(context)!==hoverKey)return;hoverKey='';delay.stop();pending=null;hide.restart();}
    function reveal(){if(!pending?.window?.bar || !hoverKey)return;selected=pending;Qt.callLater(finishReveal);}
    function finishReveal(){
        if(!selected || selected!==pending || key(selected.context)!==hoverKey)return;
        syncGeometry();controller.closeExternalPanels();opened=true;
    }
    function settleLayout(item){
        for(const child of item.children)settleLayout(child);
        if(typeof item.forceLayout==='function')item.forceLayout();
    }
    // Settle content at its destination width before animating the enclosing card.
    function syncGeometry(){
        if(!selected)return;
        implicitWidth=targetWidth;
        if(!mediaPanel && !audioPanel && !networkPanel)settleLayout(openrouterPanel?openrouterDetails:codexPanel?codexDetails:statusContent);
        implicitHeight=targetHeight;
        locate(selected.context,selected.window);
    }
    function pointerHeld(){
        const pos=attachedPosition(),side=bar?.side,w=body.width,h=body.height;
        const edge={x:pos.x+(side==='r'?w:0),y:pos.y+(side==='b'?h:0)};
        return Logic.panelHeld(Date.now()-controller.pointerUpdated<1500?controller.pointer:null,selected?.window?.screen?.name,selected?.context.hoverRegion,edge,w,h,side);
    }
    function close(){++transitionSerial;contentFade.stop();transitionImage=null;contentProgress=1;opened=false;resetDetach();pinned=false;delay.stop();hide.stop();selected=null;pending=null;hoverKey='';}
    function capture(path){body.grabToImage(result=>result.saveToFile(path));}
    function inspect(){return {card:{x:attachedPosition().x,y:attachedPosition().y,width:body.width,height:body.height},crossfade:contentProgress,surface:attachedSurface?{width:attachedSurface.width,height:attachedSurface.height}:null,opened:panelVisible,detached:detached,dragging:dragging,placement:placement(),title:panelTitle,provider:provider,monitor:panelScreen?.name || '',pinned:pinned,count:fields.length,pending:delay.running,media:mediaPanel,artworkReady:mediaDetails.artworkReady,lyricsShown:mediaDetails.lyricsShown,lyricsStatus:mediaDetails.lyricsStatus,artwork:mediaPanel?row?.media?.artwork || '':'',anchorX:anchor.rect.x,anchorY:anchor.rect.y,width:implicitWidth,height:implicitHeight,audio:audioDetails.inspect(),network:networkDetails.inspect()};}
    visible:false
    color:'transparent';grabFocus:networkDetails.needsKeyboard
    readonly property real targetWidth:Math.min(codexPanel?440:420,(selected?.window?.screen.width || 480)-24)
    readonly property real renderedHeight:body.height
    readonly property real statusFooterHeight:0
    readonly property real targetHeight:mediaPanel?Logic.previewHeight(theme,selected?.window?.screen.height):Math.min(88+(audioPanel?audioDetails.implicitHeight:networkPanel?networkDetails.implicitHeight:openrouterPanel?openrouterDetails.implicitHeight:codexPanel?codexDetails.implicitHeight:statusContent.implicitHeight),(selected?.window?.screen.height || 1080)*.65)
    anchor.edges:Edges.Top|Edges.Left
    anchor.gravity:bar?.side==='t'||bar?.side==='l'?Edges.Bottom|Edges.Right:bar?.side==='r'?Edges.Bottom|Edges.Left:Edges.Top|Edges.Right
    anchor.adjustment:PopupAdjustment.Flip|PopupAdjustment.Slide
    anchor.rect.width:0;anchor.rect.height:0
    implicitWidth:440;implicitHeight:88
    onTargetWidthChanged:Qt.callLater(syncGeometry)
    onTargetHeightChanged:Qt.callLater(syncGeometry)
    Timer {id:delay;interval:popup.options.delay ?? 300;onTriggered:popup.reveal()}
    Timer {id:hide;repeat:true;interval:Logic.panelCloseDelay(popup.options);onTriggered:if(!popup.panelShell.holding(popup) && !popup.pinned && !popup.dragging && !inside.hovered && !popup.hoverKey && !popup.pointerHeld())popup.close()}
    Connections {
        target:popup.controller
        function onBarsChanged(){if(popup.selected && !popup.controller.bars.some(b=>b.key===popup.selected.context.barKey && b.moduleConfigs[popup.selected.context.module] && (popup.pinned || Logic.detailHover(b.moduleConfigs[popup.selected.context.module],popup.row || {}))))popup.close();}
        function onVisibilityChanged(){if(!popup.detached && !popup.dragging && popup.bar && !popup.controller.shown(popup.bar))popup.close();}
    }
    Rectangle {
        id:body;parent:popup.bodyParent;opacity:popup.bodyOpacity;anchors.fill:parent;radius:12;color:popup.theme.background
        border.width:1;border.color:popup.theme.panelBorder || popup.theme.border
        HoverHandler {id:inside;blocking:false;onHoveredChanged:{if(hovered)hide.stop();else if(!popup.hoverKey)hide.restart();}}
        Item {
            id:contentStack;anchors.fill:parent;clip:true
            // Bottom anchors and clipping follow the card throughout resizing.
            Item {
                id:liveContent;width:popup.detached?body.width:popup.implicitWidth;height:body.height;opacity:popup.contentProgress;enabled:popup.contentProgress>=1
                MediaDetails {id:mediaDetails;visible:popup.mediaPanel;popup:popup;anchors.fill:parent;anchors.margins:1}
                PanelHeader {
                    id:header;x:16;y:16;width:parent.width-32;visible:!popup.mediaPanel;theme:popup.theme
                    title:popup.panelTitle
                    imageSource:({mullvad:'icons/tray/mullvad.svg',tailscale:'icons/tailscale.svg',codex:'icons/codex.svg',openrouter:'icons/openrouter.svg',audio:'icons/proicons/volume.svg',network:'icons/proicons/wi-fi.svg'})[popup.provider] || ''
                    symbol:({datetime:'schedule',locale:'keyboard',audio:'volume_up',network:'wifi'})[popup.provider] || 'info'
                    popup:popup
                }
                Rectangle {x:16;y:58;width:parent.width-32;height:1;visible:!popup.mediaPanel;color:popup.theme.surface}
                // Retain natural overflow for scrolling while these layouts resize.
                Flickable {
                    visible:popup.audioPanel;x:16;y:72;width:parent.width-32;height:parent.height-88;clip:true
                    contentWidth:width;contentHeight:audioDetails.implicitHeight;boundsBehavior:Flickable.StopAtBounds
                    AudioDetails {id:audioDetails;width:parent.width;height:Math.max(0,implicitHeight+popup.renderedHeight-popup.targetHeight);popup:popup}
                    ScrollBar.vertical:ScrollBar {policy:ScrollBar.AsNeeded}
                }
                Flickable {
                    visible:popup.networkPanel;x:16;y:72;width:parent.width-32;height:parent.height-88;clip:true
                    contentWidth:width;contentHeight:networkDetails.implicitHeight;boundsBehavior:Flickable.StopAtBounds
                    NetworkDetails {id:networkDetails;width:parent.width;height:Math.max(0,implicitHeight+popup.renderedHeight-popup.targetHeight);popup:popup}
                    ScrollBar.vertical:ScrollBar {policy:ScrollBar.AsNeeded}
                }
                Flickable {
                    visible:popup.codexPanel;x:16;y:72;width:parent.width-32;height:parent.height-88;clip:true
                    contentWidth:width;contentHeight:codexDetails.implicitHeight;boundsBehavior:Flickable.StopAtBounds
                    CodexDetails {id:codexDetails;width:parent.width;popup:popup}
                    ScrollBar.vertical:ScrollBar {policy:ScrollBar.AsNeeded}
                }
                Flickable {
                    visible:popup.openrouterPanel;x:16;y:72;width:parent.width-32;height:parent.height-88;clip:true
                    contentWidth:width;contentHeight:openrouterDetails.implicitHeight;boundsBehavior:Flickable.StopAtBounds
                    OpenRouterDetails {id:openrouterDetails;width:parent.width;popup:popup}
                    ScrollBar.vertical:ScrollBar {policy:ScrollBar.AsNeeded}
                }
                Flickable {
                    visible:!popup.mediaPanel && !popup.codexPanel && !popup.openrouterPanel && !popup.audioPanel && !popup.networkPanel
                    x:16;y:72;width:parent.width-32;height:Math.max(0,parent.height-88-popup.statusFooterHeight);clip:true
                    contentWidth:width;contentHeight:statusContent.implicitHeight;boundsBehavior:Flickable.StopAtBounds
                    Item {
                        id:statusContent;width:parent.width
                        implicitHeight:statusDetails.implicitHeight+(connectivityFooter.visible?12+connectivityFooter.implicitHeight:0)
                        StatusDetails {id:statusDetails;width:parent.width;popup:popup}
                        Column {
                            id:connectivityFooter;width:parent.width;spacing:12;visible:popup.provider==='mullvad'
                            y:statusDetails.implicitHeight+12+popup.renderedHeight-popup.targetHeight
                            MullvadLocations {id:mullvadLocations;width:parent.width;popup:popup}
                            Item {
                                width:parent.width;height:56;visible:!!popup.tailnetModule
                                PanelHeader {
                                    y:12;width:parent.width;theme:popup.theme;popup:popup
                                    title:'Tailscale';imageSource:'icons/tailscale.svg';showHandle:false
                                }
                                Rectangle {y:54;width:parent.width;height:1;color:popup.theme.surface}
                            }
                            StatusDetails {
                                width:parent.width;popup:popup;visible:!!popup.tailnetModule
                                provider:'tailscale';info:popup.tailnetRow?.panel || ({});statusRow:popup.tailnetRow
                                feedback:popup.moduleFeedback(popup.tailnetModule);actionModule:popup.tailnetModule
                            }
                        }
                    }
                    ScrollBar.vertical:ScrollBar {policy:ScrollBar.AsNeeded}
                }
            }
            Image {
                visible:!!popup.transitionImage
                width:popup.transitionWidth;height:popup.transitionHeight
                source:popup.transitionImage?popup.transitionImage.url:''
                opacity:1-popup.contentProgress
            }
        }
    }
}
