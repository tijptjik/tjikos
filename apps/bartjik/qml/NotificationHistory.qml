import QtQuick
import Quickshell.Io
import "Notifications" as Notifications
import "Logic.js" as BarLogic

Item {
    id:root
    required property var popup
    readonly property bool active:popup.opened && popup.provider==='DND'
    readonly property bool needsKeyboard:active
    readonly property bool demo:popup.controller.demo
    readonly property var theme:Object.assign({padding:12,radius:10,borderWidth:0,fontSize:14,critical:'#eb6f92',success:'#9ccf7a'},payload.config?.style || {},popup.theme)
    readonly property var config:Object.assign({components:{},grouping:{enabled:true},actions:{blacklist:[]},stack:{}},payload.config || {},{style:theme})
    property var payload:({items:[],paused:false,locked:true})
    property string message:''
    property bool hasSnapshot:false
    property double now:Date.now()
    readonly property bool locked:payload.locked!==false
    readonly property bool paused:payload.paused===true
    readonly property var avatarCache:payload.avatars || ({})
    property bool keyboardMode:true
    property string selectedId:''
    property string keyboardMonitor:''
    readonly property var telegramPlayer:player
    readonly property var telegramRecorder:recorder
    Notifications.TelegramPlayback {id:player;locked:root.locked || !root.active}
    Notifications.TelegramRecorder {id:recorder;controller:root;locked:true}
    Notifications.HistoryModel {id:model;items:root.locked?[]:root.payload.items || [];grouping:root.config.grouping?.enabled!==false}
    readonly property var historyItems:model.items
    property alias historyQuery:model.query
    readonly property bool historyFiltering:model.filtering
    readonly property var filteredHistoryItems:model.filtered
    property alias historySelectedKey:model.selected
    readonly property var historyRows:model.rows
    function historyGroupExpanded(key){return model.isExpanded(key);}
    function toggleGroup(key){model.toggle(key);}
    function selectHistory(key){model.select(key);}
    function navigate(direction){model.navigate(direction);}
    function historyCard(notice){return Object.assign({data:{},actions:[],icon:'',image:'',type:'standard',urgency:'normal',remaining:0},notice);}
    function find(id){return historyItems.find(n=>n.id===id && !n.archived) || null;}
    function targetMonitor(){return popup.panelScreen?.name || '';}
    function setHover(screen,id,hovered){}
    function sendAction(id,action,index){
        const notice=historyItems.find(n=>n.id===id && !n.archived);
        if(!notice || locked)return;
        run(['history-action',notice.historyKey,action,'--index',String(index || 0)]);
    }
    function invoke(id,index){sendAction(id,'invoke',index);}
    function close(id){sendAction(id,'dismiss',0);}
    function activateSelected(){const n=historyItems.find(n=>n.historyKey===historySelectedKey);if(n && n.actions?.length)invoke(n.id,0);}
    function dismissSelected(){const n=historyItems.find(n=>n.historyKey===historySelectedKey);if(n)close(n.id);}
    function handle(request){if(request.op==='resume' || request.op==='pause')run([request.op]);}
    function refresh(){if(active && !demo && !query.running)query.running=true;}
    function run(args){
        if(operation.running)return;
        if(demo){lastAction=args;return;}
        message='';operation.command=['notjikctl'].concat(args);operation.running=true;
    }
    property var lastAction:null
    function inspect(){return {active:active,available:!locked,query:historyQuery,matches:filteredHistoryItems.length,count:historyItems.length,selected:historySelectedKey,editing:view.editing,lastAction:lastAction};}
    function focusHistory(){view.focusHistory();}
    function reset(){model.reset();}
    onActiveChanged:{
        if(active){reset();if(demo)loadDemo();else refresh();}
        else {query.running=false;payload={items:[],paused:false,locked:true};hasSnapshot=false;message='';model.reset();}
    }
    Timer {interval:1000;repeat:true;running:root.active;onTriggered:{root.now=Date.now();root.refresh();}}
    Process {
        id:query;command:['notjikctl','history-data']
        stdout:StdioCollector {onStreamFinished:{
            if(!root.active)return;
            try {const data=JSON.parse(text);if(!data.ok)throw Error();root.payload=BarLogic.reconcile(root.payload,data);root.hasSnapshot=true;root.message='';}
            catch(_){root.payload={items:[],locked:true};root.message='Notification history unavailable';}
        }}
        onExited:(code,status)=>{if(code!==0 && root.active){root.payload={items:[],locked:true};root.message='Notification history unavailable';}}
    }
    Process {
        id:operation
        onExited:(code,status)=>{if(code!==0)root.message='This notification is no longer available';root.refresh();}
    }
    Notifications.HistoryView {
        id:view;anchors.fill:parent;anchors.bottomMargin:root.message?26:0
        controller:root;monitorName:root.targetMonitor();visible:!root.locked
        onEditingChanged:if(editing)root.popup.pinned=true
        onCloseRequested:root.popup.close()
    }
    Text {anchors.centerIn:parent;visible:root.locked && !root.message;text:root.hasSnapshot?'Notification history is locked':'Loading notifications…';font.family:root.theme.font;font.pixelSize:13;color:root.theme.muted}
    Text {anchors.bottom:parent.bottom;width:parent.width;text:root.message;visible:!!text;wrapMode:Text.Wrap;font.family:root.theme.font;font.pixelSize:12;color:root.theme.muted}
    function loadDemo(){payload={locked:false,paused:false,config:{grouping:{enabled:true}},items:[
        {id:'demo-1',historyKey:'demo/1',app:'Calendar',title:'Weekend plans',body:'Meet at the Central pier',received:Date.now()-60000,archived:true,actions:[]},
        {id:'demo-2',historyKey:'demo/2',app:'Calendar',title:'Work',body:'Design review tomorrow',received:Date.now()-120000,archived:true,actions:[]},
        {id:'demo-3',historyKey:'demo/3',app:'Mail',title:'Central booking',body:'Your reservation is confirmed',received:Date.now()-180000,archived:true,actions:[]}
    ]};}
    Component.onCompleted:if(demo)loadDemo()
}
