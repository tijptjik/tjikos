//@ pragma UseQApplication
import QtQuick
import Quickshell
import Quickshell.Io
import "Logic.js" as Logic

ShellRoot {
    id: root
    property var state: ({ready:false,config:{},bars:[],providers:{}})
    property bool workspaceReminder: false
    property var visibility: ({})
    property var edgeStates: ({})
    signal artworkReceived(string token,string source)
    signal lyricsReceived(string token,var result)
    property string helperError: ''
    property var pointer: ({valid:false})
    property double pointerUpdated: 0
    function menuOpen(key) {return (native.menuPopup.visible && native.menuPopup.anchor.window?.barKey===key) || (cryptoPanel.panelVisible && !cryptoPanel.detached && cryptoPanel.anchor.window?.barKey===key) || (detailPanel.panelVisible && !detailPanel.detached && detailPanel.anchor.window?.barKey===key) || (workspacePreview.item?.panelVisible && !workspacePreview.item?.detached && workspacePreview.item?.anchor.window?.barKey===key);}
    readonly property var bars: state.bars || []
    readonly property bool demo: Quickshell.env('BARTJIK_DEMO')==='1'
    readonly property var screenInfo: Quickshell.screens.map(s=>({name:s.name,width:s.width,height:s.height,scale:s.devicePixelRatio}))
    readonly property alias panelShell:sharedPanelShell
    PanelShell {id:sharedPanelShell}
    Native {id:native;demo:root.demo}
    CryptoPanel {id:initialCryptoPanel;controller:root}
    property var cryptoPanel:initialCryptoPanel
    property var cryptoPanels:[initialCryptoPanel]
    property int cryptoRequestSerial:0
    Component {id:cryptoPanelFactory;CryptoPanel {controller:root}}
    function availableCryptoPanel() {
        // Keep the original instance and its pending chart requests with the
        // detached card. Reuse closed cards so allocations track peak usage.
        if(cryptoPanel.detached && !cryptoPanel.dragging && !cryptoPanel.dropping){
            let next=cryptoPanels.find(p=>!p.opened && !p.dragging && !p.dropping);
            if(!next){next=cryptoPanelFactory.createObject(root);cryptoPanels=cryptoPanels.concat([next]);}
            cryptoPanel=next;
        }
        return cryptoPanel;
    }
    DetailPanel {id:detailPanel;controller:root}
    Loader {
        id:workspacePreview
        active:root.bars.some(b=>Object.values(b.moduleConfigs).some(m=>m.provider==='workspaces' && m.preview?.enabled))
        sourceComponent:Component {WorkspacePreview {controller:root}}
    }
    function workspaceEnter(context,window,row,options) {workspacePreview.item?.enter(context,window,row,options);}
    function workspaceLeave(context) {workspacePreview.item?.leave(context);}
    function cryptoEnter(context,window,row,settings) {availableCryptoPanel().enter(context,window,row,settings);}
    function detailEnter(context,window,row,settings) {detailPanel.enter(context,window,row,settings);}
    function detailLeave(context) {detailPanel.leave(context);}
    function externalLeave(context){if(!demo)send({op:'externalLeave',context:context});}
    function closeExternalPanels(){if(!demo)send({op:'closeExternalPanels'});}
    function cryptoLeave(context) {cryptoPanel.leave(context);}

    function send(message) {if(backend.running)backend.write(JSON.stringify(message)+'\n');}
    function items(bar,id) {
        const m=bar.moduleConfigs[id];if(!m)return [];
        if(!demo && (m.provider==='playerctl' || m.provider==='icontray'))return native.items(m);
        return (state.providers[bar.sources[id]] || {}).items || [];
    }
    function connectivityPeer(bar,id,provider) {
        const island=bar?.islands.find(i=>i.modules.includes(id));
        return island?.modules.find(key=>key!==id && bar.moduleConfigs[key]?.provider===provider) || '';
    }
    function displayItems(bar,id) {
        if(bar.moduleConfigs[id]?.provider==='tailscale' && connectivityPeer(bar,id,'mullvad'))return [];
        return items(bar,id);
    }
    function mediaPlayer(id) {return native.players.find(p=>p.dbusName===id) || null;}
    function shown(bar) {return visibility[bar.id]===undefined ? bar.visible : visibility[bar.id];}
    function updateEdges() {
        const now=Date.now(), sample=now-pointerUpdated<1500?pointer:{valid:false};
        const entries=panels.instances.filter(p=>p.bar && p.targetScreen && p.visible).map(p=>({
            bar:p.bar,screen:{name:p.targetScreen.name,width:p.targetScreen.width,height:p.targetScreen.height,scale:p.targetScreen.devicePixelRatio},
            hovered:p.edgeHovered,fading:p.edgeFading
        }));
        edgeStates=Logic.reconcile(edgeStates,Logic.edgeStates(edgeStates,entries,sample,now));
    }
    Timer {interval:25;running:true;repeat:true;onTriggered:root.updateEdges()}
    function act(action,context,window,row) {
        if(action==='crypto.panel') {
            availableCryptoPanel();
            if(cryptoPanel.open(context,window,row))send({op:'crypto',context:context,token:cryptoPanel.token});return;
        }
        if(['audio.panel','network.panel'].includes(action)){detailPanel.open(context,window,row);return;}
        if(demo)return;
        if(typeof action==='string' && native.act(action,row.id,window,context.anchor))return;
        send({op:'action',context:context});
    }
    function reserveDepth(connector,side) {
        let depth=0;
        panels.instances.forEach(p=>{
            if(p.bar?.screen!==connector || p.bar?.side!==side || !p.bar?.reserve || !p.visible || p.concealed)return;
            const g=p.geometry;
            const d=side==='t'?g.y+g.height:side==='b'?p.screen.height-g.y:side==='l'?g.x+g.width:p.screen.width-g.x;
            depth=Math.max(depth,d);
        });
        return depth;
    }
    function request(req) {
        switch(req.op) {
        case 'show-workspaces':workspaceReminder=true;break;
        case 'hide-workspaces':workspaceReminder=false;break;
        case 'reload':send({op:'reload'});break;
        case 'toggle-reveal': case 'show-reveal': case 'hide-reveal': {
            if(req.bar!=='all' && !bars.some(b=>b.id===req.bar))throw Error('unknown bar');
            const next=Object.assign({},visibility), edges=Object.assign({},edgeStates);
            const selected=bars.filter(b=>req.bar==='all' || req.bar===b.id);
            const held=selected.some(b=>edges[b.key]?.keyboardHeld);
            selected.forEach(b=>{
                next[b.id]=true;
                edges[b.key]=Logic.toggleReveal({keyboardHeld:req.op==='hide-reveal' || (req.op==='toggle-reveal' && held)});
            });
            edgeStates=edges;visibility=next;break;
        }
        case 'show': case 'hide':case 'toggle':
            if(req.bar!=='all' && !bars.some(b=>b.id===req.bar))throw Error('unknown bar');
            const next=Object.assign({},visibility);
            const edges=Object.assign({},edgeStates);
            bars.forEach(b=>{
                if(req.bar!=='all' && req.bar!==b.id)return;
                next[b.id]=req.op==='toggle'?!shown(b):req.op==='show';
                // Explicit show must also reveal an already-enabled auto-hidden bar.
                if(next[b.id])edges[b.key]={concealed:false,enter:null,leave:null};
            });
            edgeStates=edges;visibility=next;break;
        case 'config':return {ok:true,config:state.config,bars:bars,error:state.error};
        case 'monitors':return {ok:true,monitors:screenInfo};
        case 'bars':return {ok:true,bars:panels.instances.map(p=>p.inspect())};
        case 'modules':return {ok:true,modules:[].concat(...bars.map(b=>Object.keys(b.moduleConfigs).map(id=>({bar:b.id,monitor:b.screen,id:id,provider:b.moduleConfigs[id].provider,source:b.sources[id],items:items(b,id)})))),providers:state.providers,native:native.snapshot(),compositor:state.compositor || {}};
        case 'cryptoPreview':
            if(!demo)throw Error('preview requires demo mode');
            const cp=panels.instances.find(p=>p.bar.moduleConfigs[req.module]?.provider==='crypto');
            if(!cp)throw Error('crypto module missing');
            const row=items(cp.bar,req.module).find(r=>r.id===req.item);if(!row)throw Error('unknown item');
            act('crypto.panel',{barKey:cp.bar.key,module:req.module,monitor:cp.bar.screen,item:row.id,anchor:{x:cp.geometry.x,y:cp.geometry.y,width:20,height:28}},cp,row);
            return {ok:true};
        case 'workspacePreviewState':return {ok:true,preview:workspacePreview.item?.inspect() || {opened:false}};
        case 'workspacePreviewTest':
            if(!demo)throw Error('preview testing requires demo mode');
            const wp=panels.instances.find(p=>p.bar.moduleConfigs[req.module]?.provider==='workspaces' && (!req.monitor || p.bar.screen===req.monitor));
            const wr=items(wp.bar,req.module).find(r=>r.id===req.item);
            const wc={barKey:wp.bar.key,module:req.module,item:wr.id,monitor:wp.bar.screen,anchor:{x:req.x,y:wp.geometry.y,width:20,height:28}};
            if(req.leave)workspaceLeave(wc);else workspaceEnter(wc,wp,wr,wp.bar.moduleConfigs[req.module].preview);
            return {ok:true};
        case 'captureWorkspace':
            if(!demo)throw Error('capture requires demo mode');
            workspacePreview.item.capture(req.path);return {ok:true};
        case 'cryptoHoverTest':
            if(!demo)throw Error('hover testing requires demo mode');
            const hp=panels.instances.find(p=>p.bar.moduleConfigs[req.module]?.provider==='crypto');
            const hr=items(hp.bar,req.module).find(r=>r.id===req.item);
            const hc={barKey:hp.bar.key,module:req.module,monitor:hp.bar.screen,item:hr.id,part:req.part || 'icon',anchor:{x:req.x ?? hp.geometry.x,y:hp.geometry.y,width:20,height:hp.geometry.height}};
            if(req.leave)cryptoLeave(hc);else cryptoEnter(hc,hp,hr,hp.bar.moduleConfigs[req.module].panel);
            return {ok:true};
        case 'detailState':return {ok:true,panel:detailPanel.inspect()};
        case 'detailTest':
            if(!demo)throw Error('panel testing requires demo mode');
            const dp=panels.instances.find(p=>p.bar.islands.some(i=>i.modules.includes(req.module)));
            const dr=items(dp.bar,req.module)[0];
            const dc={barKey:dp.bar.key,module:req.module,item:dr.id,monitor:dp.bar.screen,anchor:{x:req.x || 700,y:dp.geometry.y,width:0,height:0}};
            if(req.leave)detailLeave(dc);else detailEnter(dc,dp,dr,dp.bar.moduleConfigs[req.module].panel);
            return {ok:true};
        case 'captureDetail':if(!demo)throw Error('capture requires demo mode');detailPanel.capture(req.path);return {ok:true};
        case 'cryptoState':return {ok:true,pair:cryptoPanel.pair,opened:cryptoPanel.panelVisible,detached:cryptoPanel.detached,dragging:cryptoPanel.dragging,placement:cryptoPanel.placement(),points:cryptoPanel.points.length,error:cryptoPanel.chart.error || '',anchorX:cryptoPanel.anchor.rect.x,anchorY:cryptoPanel.anchor.rect.y,width:cryptoPanel.implicitWidth,height:cryptoPanel.implicitHeight,pending:cryptoPanel.waiting,pinned:cryptoPanel.pinned};
        case 'captureCrypto':
            if(!demo)throw Error('capture requires demo mode');
            cryptoPanel.capture(req.path);return {ok:true};
        case 'capture':
            if(!demo)throw Error('capture requires synthetic demo mode');
            const p=panels.instances.find(p=>p.barKey===req.bar);if(!p)throw Error('unknown bar key');p.capture(req.path);return {ok:true};
        case 'status':break;
        default:throw Error('unknown operation');
        }
        return {ok:true,ready:state.ready,demo:demo,theme:state.config?.theme,error:state.error || helperError,
            workspaceReminder:workspaceReminder,pointerError:pointer.error || '',actionError:native.navigationError || state.actionError,bars:bars.length,providers:Object.keys(state.providers || {}).length};
    }
    IpcHandler {
        target:'bartjik'
        function request(payload:string):string {
            try{return JSON.stringify(root.request(JSON.parse(payload)));}
            catch(e){return JSON.stringify({ok:false,error:String(e)});}
        }
    }
    Process {
        id:backend
        command:['python3','-B',Quickshell.env('BARTJIK_HOME')+'/bartjik.py','serve'].concat(root.demo?['--demo']:[])
        running:true
        stdinEnabled:true
        stdout:SplitParser {onRead:data=>{
            try {const msg=JSON.parse(data);if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}else if(msg.kind==='crypto'){root.cryptoPanels.forEach(p=>p.receive(msg.token,msg.chart));}else if(msg.kind==='artwork'){root.artworkReceived(msg.token,msg.source);}else if(msg.kind==='lyrics'){root.lyricsReceived(msg.token,msg.lyrics);}else root.state=Logic.reconcile(root.state,msg);root.helperError='';}
            catch(e){root.helperError='Invalid helper JSON';}
        }}
        onStarted:root.send({op:'screens',screens:root.screenInfo})
        onExited:(code,status)=>{root.helperError='Provider helper exited ('+code+')';retry.restart();}
    }
    Timer {id:retry;interval:1000;onTriggered:backend.running=true}
    onScreenInfoChanged:send({op:'screens',screens:screenInfo})
    Variants {
        id:panels
        model:root.bars.map(b=>b.key)
        Bar {required property string modelData;barKey:modelData;controller:root}
    }
    Variants {
        model:root.bars.filter(b=>b.islands.some(i=>i.modules.some(id=>b.moduleConfigs[id]?.provider==='workspaces'))).map(b=>b.key)
        WorkspaceReminder {required property string modelData;barKey:modelData;controller:root}
    }
    Variants {
        model:[...new Set(root.bars.filter(b=>b.reserve).map(b=>b.screen+'/'+b.side))]
        Reserve {required property string modelData;key:modelData;controller:root}
    }
}
