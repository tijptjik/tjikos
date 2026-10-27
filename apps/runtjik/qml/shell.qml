//@ pragma UseQApplication
import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import Quickshell.Hyprland
import Quickshell.Wayland

ShellRoot {
    id: root
    property var settings: ({ui:{width:720,height:600,columns:5,icon_size:52,font:'tijpset',font_size:14,monitor:'focused',keep_alive:true},theme:{background:'#191724',surface:'#26233a',tile:'#1f1d2e',text:'#e0def4',muted:'#908caa',accent:'#ff53a6',border:'#403a52'},index:{refresh_seconds:5}})
    readonly property var theme: settings.theme
    readonly property bool demo: Quickshell.env('RUNTJIK_DEMO') === '1'
    property var closingIds: []
    ListModel {id: resultModel; dynamicRoles: true}
    property bool animateClosedRemoval: false
    // Preserve delegates by identity; animate only closed apps leaving the same running query.
    onResultsChanged: {
        const wanted=new Set(results.map(row=>row.id));
        const removed=[];
        for(let i=0;i<resultModel.count;++i){
            const row=resultModel.get(i).row;
            if(!wanted.has(row.id))removed.push(row);
        }
        animateClosedRemoval=opened && runningMode && resultQuery===query && windowEntry===''
            && removed.length>0 && removed.every(row=>row.provider==='apps' && row.windows>0);
        for(let i=resultModel.count-1;i>=0;--i)
            if(!wanted.has(resultModel.get(i).row.id))resultModel.remove(i);
        for(let i=0;i<results.length;++i){
            let found=i;
            while(found<resultModel.count && resultModel.get(found).row.id!==results[i].id)++found;
            if(found===resultModel.count)resultModel.insert(i,{row:results[i]});
            else {if(found!==i)resultModel.move(found,i,1);resultModel.setProperty(i,'row',results[i]);}
        }
    }
    property var results: []
    property string runtimeVersion: ''
    property string view: 'grid'
    property string windowEntry: ''
    property string message: ''
    property int indexed: 0
    property int total: 0
    property int selected: 0
    property int serial: 0
    property bool ready: false
    property bool busy: false
    property bool quitting: false
    property bool opened: true
    property var homeResults: []
    property bool centering: false
    property bool transferring: false
    property bool dragging: false
    property real dragX: 0
    property real dragY: 0
    property var dropScreen: null
    property point transferPosition
    property string query: ''
    property string resultQuery: ''
    property var pendingAction: null
    property var queuedRequest: null
    property bool hasMore: false
    property bool loadingMore: false
    property bool applyingResults: false
    property int nextOffset: 0
    property int pendingSelection: -1
    readonly property bool runMode: query === '/run' || query.startsWith('/run ') || query.startsWith('!')
    readonly property bool runningMode: ['/running','/processes'].some(prefix=>query===prefix || query.startsWith(prefix+' '))
    readonly property bool listMode: view === 'list' || windowEntry !== '' || (query.startsWith('/') && !runMode && !runningMode)
    // Capture the focused screen once; keyboard navigation must not move the panel.
    property string movedMonitor: ''
    property string initialMonitor: ''
    property bool initialCaptured: false
    readonly property string focusedMonitorName: Hyprland.focusedMonitor?.name || ''
    onFocusedMonitorNameChanged: captureInitialMonitor()
    function captureInitialMonitor() {
        if(!initialCaptured && ready && (focusedMonitorName || settings.ui.monitor!=='focused')){
            initialMonitor=focusedMonitorName;initialCaptured=true;
            if(opened)panel.visible=true;
        }
    }
    function send(data) {
        if (!backend.running) return;
        data.serial = ++serial;
        backend.write(JSON.stringify(data) + '\n');
    }
    function search() {
        if (!windowEntry && !busy && !loadingMore) {
            const limit=resultQuery===query?Math.max(results.length,settings.ui.max_results || 100):(settings.ui.max_results || 100);
            send({op:'query',query:query,limit:limit});
        }
    }
    function loadMore() {
        if(!opened || !hasMore || loadingMore || applyingResults || busy || windowEntry || debounce.running || resultQuery!==query)return;
        loadingMore=true;
        send({op:'query',query:query,offset:nextOffset});
    }
    function maybeLoadMore() {
        if(items.height>0 && items.contentHeight>0 && items.contentY+items.height>=items.contentHeight-items.cellHeight*2)loadMore();
    }
    function move(delta) {
        if (!results.length) return;
        const target=(pendingSelection>=0?pendingSelection:selected)+delta;
        if(delta>0 && target>=results.length && hasMore){pendingSelection=target;loadMore();return;}
        selected = (target + results.length) % results.length;
        items.positionViewAtIndex(selected, GridView.Contain);
        if(delta>0 && selected>=results.length-2*settings.ui.columns)loadMore();
    }
    function focusSearch() {
        if(!opened || quitting)return;
        panel.acquireKeyboard();
        searchBox.forceActiveFocus();
    }
    function activate(switchWindow, closeWindow=false) {
        if (busy || !results[selected]) return;
        if (results[selected].disabled) return;
        if (!windowEntry && (debounce.running || resultQuery !== query)) {
            debounce.stop();pendingAction={switchWindow:switchWindow,closeWindow:closeWindow};search();return;
        }
        busy = true;
        const request=windowEntry?{op:'activate',id:windowEntry,mode:closeWindow?'close':'switch',address:results[selected].id}
            :{op:'activate',id:results[selected].id,mode:closeWindow?'close':switchWindow?'switch':'launch'};
        request.prepare=switchWindow && !closeWindow && !windowEntry && ['apps','commands'].includes(results[selected].provider);
        if (!demo && !closeWindow && !closingIds.includes(request.id) && !request.prepare && (windowEntry || !results[selected].id.startsWith('/'))) {
            queuedRequest=request;
            if(panel.pinned){panel.ownsKeyboard=false;panel.acquiringKeyboard=false;searchBox.focus=false;}
            else panel.visible=false;
            actionDelay.restart();
        } else send(request);
    }
    function quit() {
        dragging=false;
        quitting = true;
        panel.visible = false;
        backend.running = false;
        Qt.quit();
    }
    function close() {
        if(demo || !settings.ui.keep_alive){quit();return;}
        opened=false;
        panel.visible=false;
        debounce.stop();actionDelay.stop();
        pendingAction=null;queuedRequest=null;
        loadingMore=false;pendingSelection=-1;
        busy=false;serial++;
    }
    function open() {
        if(opened)return;
        initialMonitor=Hyprland.focusedMonitor?.name || '';
        movedMonitor='';transferring=false;centering=true;
        panel.pinned=false;panel.ownsKeyboard=true;windowEntry='';query='';searchBox.text='';
        message='';selected=0;pendingAction=null;busy=false;
        loadingMore=false;hasMore=false;nextOffset=0;pendingSelection=-1;
        results=homeResults;resultQuery='';view=settings.ui.view;items.positionViewAtBeginning();
        opened=true;
        debounce.stop();
        panel.visible=true;
        searchBox.forceActiveFocus();
        send({op:'resume'});
    }
    function back() {
        if (windowEntry) {windowEntry='';query='';searchBox.text='';search();}
        else close();
    }
    function receive(msg) {
        if (actionDelay.running && msg.kind==='results') return;
        if (msg.serial && msg.serial < serial) return;
        if(msg.kind==='results' && ready && (msg.query || '')!==query && !msg.reset_query && msg.edit_query===undefined)return;
        if (msg.kind !== 'results' || msg.serial === serial) busy = false;
        if (msg.kind === 'results') {
            closingIds=msg.closing || [];
            applyingResults=true;
            const appending=(msg.offset || 0)>0 && resultQuery===(msg.query || '');
            const sameQuery=resultQuery===(msg.query || '');
            const previousY=items.contentY;
            const previousId=results[selected]?.id;
            runtimeVersion=msg.version || '';
            if (msg.reset_query) {root.query='';searchBox.text='';debounce.stop();}
            if (msg.edit_query!==undefined) {root.query=msg.edit_query;searchBox.text=msg.edit_query;debounce.stop();}
            if (!ready || msg.apply_config) view=msg.config.ui.view;
            settings=msg.config;
            if(appending){
                const known=new Set(results.map(row=>row.id));
                results=results.concat(msg.results.filter(row=>!known.has(row.id)));
            } else results=msg.results;
            loadingMore=false;hasMore=!!msg.has_more;nextOffset=msg.next_offset ?? results.length;
            indexed=msg.indexed;total=msg.total ?? results.length;
            resultQuery=msg.query || '';
            if(resultQuery==='')homeResults=msg.results;
            const previousIndex=sameQuery?results.findIndex(row=>row.id===previousId):-1;
            selected=previousIndex>=0?previousIndex:Math.min(selected,Math.max(0,results.length-1));
            if(appending && pendingSelection>=0){selected=Math.min(pendingSelection,results.length-1);pendingSelection=-1;Qt.callLater(()=>items.positionViewAtIndex(selected,GridView.Contain));}
            else if(sameQuery)Qt.callLater(()=>{items.contentY=Math.max(0,Math.min(previousY,items.contentHeight-items.height));root.maybeLoadMore();});
            message=msg.message || (msg.warnings || []).join(' · ');
            ready=true;captureInitialMonitor();
            applyingResults=false;
            if (pendingAction!==null) {const action=pendingAction;pendingAction=null;Qt.callLater(()=>root.activate(action.switchWindow,action.closeWindow));}
        } else if (msg.kind === 'edit') {
            searchBox.text=msg.text;
            searchBox.forceActiveFocus();
            if(msg.select_value) {const start=msg.text.indexOf(' ',8)+1;searchBox.select(start,msg.text.length);}
        } else if (msg.kind === 'launch') {
            busy=true;
            queuedRequest={op:'activate',id:msg.entry,mode:'launch'};
            if(demo){const request=queuedRequest;queuedRequest=null;send(request);}
            else {panel.visible=false;actionDelay.restart();}
        } else if (msg.kind === 'windows') {
            windowEntry=msg.entry;results=msg.results;selected=0;
            message='Choose a window · Enter to switch · Esc to go back';
            if(msg.auto_select)Qt.callLater(()=>root.activate(true));
        } else if (msg.kind === 'pin') {
            panel.pinned=!panel.pinned;query='';searchBox.text='';search();
        } else if (msg.kind === 'view') {
            view=msg.view;query='';searchBox.text='';search();
        } else if (msg.kind === 'activated') {
            if(msg.action==='close' && !demo){
                closingIds=[...new Set(closingIds.concat(msg.entry || []))];
                windowEntry='';busy=false;panel.visible=true;
                message=msg.message || 'Closing…';
                closePoll.restart();
            } else if (demo) {message=msg.message || 'Demo action completed';busy=false;}
            else if(panel.pinned){
                windowEntry='';busy=false;
                panel.visible=true;
                search();
            } else close();
        } else {loadingMore=false;message=msg.message || 'Unable to complete action';if(!root.ready)console.error(message);}
        if (msg.kind!=='activated' && msg.kind!=='launch' && !root.quitting && root.opened && root.initialCaptured) {panel.visible=true;if(panel.ownsKeyboard)searchBox.forceActiveFocus();}
    }
    Process {
        id: backend
        command: ['python3','-B',Quickshell.env('RUNTJIK_HOME')+'/runtjik.py','serve'].concat(root.demo?['--demo']:[])
        running: true
        stdinEnabled: true
        stdout: SplitParser {onRead: data => {
            try {root.receive(JSON.parse(data));}
            catch(e) {root.message='Invalid backend response: '+String(e);root.busy=false;}
        }}
        onExited: (code,status) => {if(!root.quitting){console.error('Runtjik backend exited ('+code+'). Check runtjik.conf.');root.quit();}}
    }
    Timer {id:actionDelay;interval:80;onTriggered:{const request=root.queuedRequest;root.queuedRequest=null;root.send(request);}}
    Timer {id:closePoll;interval:180;repeat:true;running:root.opened && root.closingIds.length>0;onTriggered:root.search()}
    Timer {id:debounce;interval:60;onTriggered:root.search()}
    Timer {interval:root.settings.index.refresh_seconds*1000;running:root.ready && root.opened;repeat:true;onTriggered:root.search()}
    IpcHandler {
        target: 'runtjik'
        function close(): void {root.close();}
        function releaseKeyboard(): void {if(panel.pinned){panel.ownsKeyboard=false;panel.acquiringKeyboard=false;searchBox.focus=false;}}
        function outsideClick(): void {
            // Consult current UI state even if the passive observer is still rearming.
            if(panel.pinned){panel.ownsKeyboard=false;panel.acquiringKeyboard=false;searchBox.focus=false;}
            else root.close();
        }
        function focusSearch(): void {root.focusSearch();}
        function quit(): void {root.quit();}
        function reopen(): void {root.open();}
        function toggle(): void {if(root.opened)root.close();else root.open();}
        function status(): string {return JSON.stringify({version:root.runtimeVersion,pid:Quickshell.processId,layer:"top",pinned:panel.pinned,keyboardOwned:panel.ownsKeyboard,monitor:panel.screen.name,visible:root.opened && panel.visible,ready:root.ready && (root.opened ? panel.placed : !panel.backingWindowVisible),view:root.view,selected:root.selected,results:root.results.length,total:root.total,hasMore:root.hasMore,loadingMore:root.loadingMore,indexed:root.indexed,closing:root.closingIds,windowPicker:root.windowEntry!=='',message:root.message,demo:root.demo,address:panel.address,placementError:panel.placementError,clickWatchError:panel.clickWatchError,geometry:{x:panel.screen.x+frame.x,y:panel.screen.y+frame.y,width:frame.width,height:frame.height}});}
        function refresh(): void {root.send({op:'activate',id:'/refresh'});}
        function capture(path: string): void {
            if(!root.demo)throw Error('Screenshots for documentation require synthetic --demo data');
            frame.grabToImage(image=>image.saveToFile(path));
        }
        function testState(): string {
            if(!root.demo)throw Error('Test state requires --demo');
            return JSON.stringify({resultQuery:root.resultQuery,scrollY:items.contentY,rows:root.results});
        }
        function test(query: string): void {
            if(!root.demo)throw Error('Test control requires --demo');
            searchBox.text=query;
        }
    }
    PanelWindow {
        visible: root.dragging && root.dropScreen!==null && root.dropScreen.name!==panel.screen.name
        screen: root.dropScreen || panel.screen
        anchors {top:true;bottom:true;left:true;right:true}
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.layer: WlrLayer.Top
        WlrLayershell.namespace: 'runtjik-drag'
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
        color:'transparent'
        mask: Region {}
        Rectangle {
            width:Math.min(root.settings.ui.width,parent.width-32)
            height:Math.min(root.settings.ui.height,parent.height-32)
            x:Math.max(0,Math.min(root.dragX-(root.dropScreen?.x || 0),parent.width-width))
            y:Math.max(0,Math.min(root.dragY-(root.dropScreen?.y || 0),parent.height-height))
            radius:22;color:root.theme.surface;opacity:.85
            border.width:2;border.color:root.theme.accent
            Text {anchors.centerIn:parent;text:'Release to move Runtjik here';color:root.theme.text;font.family:root.settings.ui.font;font.pixelSize:16}
        }
    }
    LauncherWindow {
        id: panel
        visible: root.opened && root.ready && root.initialCaptured && panel.styled
        targetScreen: Quickshell.screens.find(s=>s.name === (root.movedMonitor || (root.settings.ui.monitor === 'focused'?root.initialMonitor:root.settings.ui.monitor))) || Quickshell.screens[0]
        color: 'transparent'
        onClosed: {if(!root.transferring)root.close();}
        onPlacedChanged: {
            if(placed && root.centering){
                frame.x=(panel.width-frame.width)/2;
                frame.y=(panel.height-frame.height)/2;
                panel.updateBounds(frame.x,frame.y,frame.width,frame.height);
                root.centering=false;
                searchBox.forceActiveFocus();
            }
            if(placed && root.transferring){
                frame.x=Math.max(0,Math.min(root.transferPosition.x,panel.width-frame.width));
                frame.y=Math.max(0,Math.min(root.transferPosition.y,panel.height-frame.height));
                panel.updateBounds(frame.x,frame.y,frame.width,frame.height);
                root.transferring=false;
                searchBox.forceActiveFocus();
            }
        }
        mask: Region {x:frame.x;y:frame.y;width:frame.width;height:frame.height;radius:22}
        Rectangle {
            id: frame
            x: (panel.width-width)/2
            y: (panel.height-height)/2
            width: Math.min(root.settings.ui.width,panel.width-32)
            height: Math.min(root.settings.ui.height,panel.height-32)
            onXChanged: boundsTimer.restart()
            onYChanged: boundsTimer.restart()
            onWidthChanged: boundsTimer.restart()
            onHeightChanged: boundsTimer.restart()
            Timer {id:boundsTimer;interval:0;onTriggered:{
                if(!root.dragging && panel.width>0 && panel.height>0){
                    if(frame.x<0 || frame.x+frame.width>panel.width)frame.x=Math.max(0,Math.min(frame.x,panel.width-frame.width));
                    if(frame.y<0 || frame.y+frame.height>panel.height)frame.y=Math.max(0,Math.min(frame.y,panel.height-frame.height));
                }
                panel.updateBounds(frame.x,frame.y,frame.width,frame.height);
            }}
            color: root.theme.surface
            radius: 22
            border.width: 1
            border.color: root.theme.border
            opacity: panel.placed ? 1 : 0
            MouseArea {
                objectName: 'windowDragArea'
                anchors.fill: parent
                cursorShape: Qt.OpenHandCursor
                property point pressPoint
                property point startPoint
                onPressed: mouse=>{
                    root.dragging=true;root.dropScreen=panel.screen;
                    pressPoint=mapToItem(panel.contentItem,mouse.x,mouse.y);
                    startPoint=Qt.point(frame.x,frame.y);
                }
                onPositionChanged: mouse=>{
                    if(!pressed)return;
                    const point=mapToItem(panel.contentItem,mouse.x,mouse.y);
                    frame.x=startPoint.x+point.x-pressPoint.x;
                    frame.y=startPoint.y+point.y-pressPoint.y;
                    root.dragX=panel.screen.x+frame.x;root.dragY=panel.screen.y+frame.y;
                    const gx=panel.screen.x+point.x,gy=panel.screen.y+point.y;
                    root.dropScreen=Quickshell.screens.find(s=>gx>=s.x && gx<s.x+s.width && gy>=s.y && gy<s.y+s.height) || null;
                }
                onReleased: mouse=>{
                    // Keep the original surface and pointer grab until release.
                    // Moving the layer mid-drag would cancel its Wayland grab.
                    const point=mapToItem(panel.contentItem,mouse.x,mouse.y);
                    const gx=panel.screen.x+point.x, gy=panel.screen.y+point.y;
                    root.dragging=false;
                    const target=Quickshell.screens.find(s=>gx>=s.x && gx<s.x+s.width && gy>=s.y && gy<s.y+s.height);
                    if(target && target.name!==panel.screen.name){
                        const x=gx-target.x-(pressPoint.x-startPoint.x);
                        const y=gy-target.y-(pressPoint.y-startPoint.y);
                        root.transferPosition=Qt.point(x,y);
                        root.transferring=true;
                        // Finish the pointer release before recreating the surface.
                        Qt.callLater(()=>{
                            panel.visible=false;
                            panel.observed=false;
                            root.movedMonitor=target.name;
                            Qt.callLater(()=>{
                                panel.visible=true;
                                searchBox.forceActiveFocus();
                            });
                        });
                    } else boundsTimer.restart();
                }
                onCanceled: {root.dragging=false;boundsTimer.restart();}
            }
            Column {
                anchors.fill: parent
                anchors.margins: 20
                anchors.bottomMargin: 8
                spacing: 14
                Row {
                    width: parent.width
                    height: 46
                    spacing: 12
                    Text {
                        width: 28;height:parent.height
                        text:'⌘';font.pixelSize:28;color:root.theme.accent
                        horizontalAlignment:Text.AlignHCenter;verticalAlignment:Text.AlignVCenter
                    }
                    Rectangle {
                        width: parent.width-40;height:parent.height;radius:23
                        color:root.theme.background;border.width:1;border.color:root.theme.border
                        TextInput {
                            id: searchBox
                            anchors.fill:parent;anchors.leftMargin:18;anchors.rightMargin:18
                            verticalAlignment:TextInput.AlignVCenter
                            color:root.theme.text;selectionColor:root.theme.accent
                            font.family:root.settings.ui.font;font.pixelSize:root.settings.ui.font_size+1
                            focus:true;clip:true;selectByMouse:true
                            readOnly:root.windowEntry!==''
                            onTextChanged:{root.pendingAction=null;root.loadingMore=false;root.pendingSelection=-1;root.query=text;root.selected=0;if(!root.windowEntry)debounce.restart();}
                            Text {
                                anchors.fill:parent;verticalAlignment:Text.AlignVCenter
                                text:root.windowEntry?'Choose a running window…':'A beginning is the time for taking the most delicate care…'
                                visible:searchBox.text.length===0
                                font:searchBox.font;color:root.theme.muted
                                fontSizeMode:Text.HorizontalFit;minimumPixelSize:11;elide:Text.ElideRight
                            }
                            Keys.onPressed:event=>{
                                if(event.key===Qt.Key_Tab || event.key===Qt.Key_Backtab){root.move((event.modifiers & Qt.ShiftModifier || event.key===Qt.Key_Backtab)?-1:1);event.accepted=true;}
                                else if(event.key===Qt.Key_Down){root.move(root.listMode?1:root.settings.ui.columns);event.accepted=true;}
                                else if(event.key===Qt.Key_Up){root.move(root.listMode?-1:-root.settings.ui.columns);event.accepted=true;}
                                else if((event.key===Qt.Key_Left || event.key===Qt.Key_Right) && (!root.listMode || (event.modifiers & Qt.ControlModifier))){root.move(event.key===Qt.Key_Left?-1:1);event.accepted=true;}
                                else if(event.key===Qt.Key_Return || event.key===Qt.Key_Enter){root.activate(!!(event.modifiers & Qt.ShiftModifier),!!(event.modifiers & Qt.ControlModifier));event.accepted=true;}
                                else if(event.key===Qt.Key_Escape){if(!event.isAutoRepeat && !(event.modifiers & Qt.MetaModifier))root.back();event.accepted=true;}
                                else if(event.key===Qt.Key_L && (event.modifiers & Qt.ControlModifier)){root.view=root.view==='grid'?'list':'grid';event.accepted=true;}
                            }
                        }
                    }
                }
                Row {
                    width:parent.width;height:24;spacing:8
                    Text {
                        width:parent.width-(pinStatus.visible?pinStatus.width+parent.spacing:0);height:parent.height
                        text:root.windowEntry?'RUNNING WINDOWS':(root.query.startsWith('/config')?'SETTINGS':(['/shortcuts','/bindings'].includes(root.query.trim())?'SHORTCUTS':(root.runningMode?'RUNNING APPLICATIONS':root.runMode?'RUN COMMANDS':(root.query?'SEARCH RESULTS':'APPLICATIONS'))))
                        font.family:root.settings.ui.font;font.pixelSize:11;font.letterSpacing:1.8
                        verticalAlignment:Text.AlignVCenter;color:root.theme.muted
                    }
                    Text {
                        id:pinStatus
                        visible:panel.pinned
                        width:implicitWidth;height:parent.height
                        text:'Pinned';color:root.theme.accent
                        font.family:root.settings.ui.font;font.pixelSize:11
                        verticalAlignment:Text.AlignVCenter
                    }
                }
                Column {
                    width:parent.width;height:parent.height-98;spacing:4
                    Rectangle {
                        width:parent.width;height:parent.height-footer.height-4
                        radius:14;color:root.theme.background
                        GridView {
                            id:items
                            anchors.fill:parent;anchors.margins:8
                            clip:true;model:resultModel
                            remove: Transition {
                                enabled:root.animateClosedRemoval
                                ParallelAnimation {
                                    NumberAnimation {property:'opacity';to:0;duration:180}
                                    NumberAnimation {property:'scale';to:.65;duration:180;easing.type:Easing.InCubic}
                                }
                            }
                            removeDisplaced: Transition {
                                enabled:root.animateClosedRemoval
                                NumberAnimation {properties:'x,y';duration:220;easing.type:Easing.OutCubic}
                            }
                            cellWidth:root.listMode?width:width/root.settings.ui.columns
                            cellHeight:root.listMode?70:122
                            currentIndex:root.selected
                            boundsBehavior:Flickable.StopAtBounds
                            onContentYChanged: root.maybeLoadMore()
                            ScrollBar.vertical:ScrollBar {policy:ScrollBar.AsNeeded}
                            delegate:Rectangle {
                                required property var row
                                readonly property var modelData: row
                                readonly property bool closing: root.closingIds.includes(modelData.id)
                                required property int index
                                width:root.listMode?items.cellWidth:items.cellWidth-6;height:items.cellHeight-6;radius:12
                                color:index===root.selected?root.theme.surface:(hover.containsMouse?Qt.rgba(1,1,1,.018):'transparent')
                                border.width:index===root.selected?1:0
                                border.color:root.theme.accent
                                property string iconSource: {
                                    const icon=modelData.icon || 'application-x-executable';
                                    if(icon.startsWith('/'))return 'file://'+icon;
                                    const found=Quickshell.iconPath(icon,true);
                                    return found || Quickshell.iconPath('application-x-executable',true);
                                }
                                Image {
                                    id:art
                                    SequentialAnimation on opacity {
                                        running: art.parent.closing
                                        loops: Animation.Infinite
                                        NumberAnimation {to:.3;duration:450;easing.type:Easing.InOutSine}
                                        NumberAnimation {to:1;duration:450;easing.type:Easing.InOutSine}
                                        onRunningChanged: if(!running)art.opacity=1
                                    }
                                    x:root.listMode?24:(parent.width-width)/2;y:root.listMode?(parent.height-height)/2:13
                                    width:root.listMode?40:root.settings.ui.icon_size;height:width
                                    source:parent.iconSource;sourceSize.width:width*2;sourceSize.height:height*2
                                    fillMode:Image.PreserveAspectFit;asynchronous:true
                                }
                                Text {
                                    visible:art.status===Image.Error || art.source.toString()===''
                                    x:art.x;y:art.y;width:art.width;height:art.height
                                    text:modelData.name.slice(0,1).toUpperCase();color:root.theme.accent;font.pixelSize:28
                                    horizontalAlignment:Text.AlignHCenter;verticalAlignment:Text.AlignVCenter
                                }
                                Rectangle {
                                    visible:root.listMode && modelData.windows>0
                                    x:8;y:(parent.height-height)/2
                                    width:7;height:7;radius:4;color:root.theme.accent
                                }
                                Item {
                                    id:titleArea
                                    x:root.listMode?78:7;y:root.listMode?12:76
                                    width:parent.width-x-(root.listMode?14:7)
                                    height:label.height
                                    Row {
                                        id:titleRow
                                        x:root.listMode?0:(parent.width-width)/2
                                        width:Math.min(parent.width,label.implicitWidth+(runningDot.visible?13:0))
                                        spacing:6
                                        Rectangle {
                                            id:runningDot
                                            visible:!root.listMode && modelData.windows>0
                                            width:7;height:7;radius:4;color:root.theme.accent
                                            y:(titleMetrics.height-height)/2
                                        }
                                        Text {
                                            id:label
                                            width:titleRow.width-(runningDot.visible?13:0)
                                            text:modelData.name;color:root.theme.text;font.family:root.settings.ui.font
                                            font.pixelSize:root.settings.ui.font_size-1
                                            horizontalAlignment:root.listMode?Text.AlignLeft:Text.AlignHCenter
                                            wrapMode:Text.Wrap;elide:Text.ElideRight;maximumLineCount:2
                                        }
                                        FontMetrics {id:titleMetrics;font:label.font}
                                    }
                                }
                                Text {
                                    visible:root.listMode
                                    x:titleArea.x;y:titleArea.y+titleArea.height+4;width:titleArea.width
                                    text:modelData.subtitle
                                    color:root.theme.muted;font.family:root.settings.ui.font;font.pixelSize:10
                                    elide:Text.ElideRight
                                }
                                MouseArea {
                                    id:hover;anchors.fill:parent;hoverEnabled:true;cursorShape:modelData.disabled?Qt.ArrowCursor:Qt.PointingHandCursor
                                    onClicked:mouse=>{root.selected=index;root.activate(!!(mouse.modifiers & Qt.ShiftModifier));}
                                }
                            }
                            Text {
                                anchors.centerIn:parent;visible:root.results.length===0
                                text:root.ready?'No matches. Try another name or /refresh.':'Building your application index…'
                                color:root.theme.muted;font.pixelSize:13
                            }
                        }
                    }
                    Item {
                        id:footer
                        width:parent.width;height:footerMessage.visible?16:12
                        Item {
                            anchors.fill:parent
                            visible:!footerMessage.visible
                            Row {
                                anchors.centerIn:parent;spacing:2
                                Repeater {
                                    model:5
                                    Rectangle {
                                        required property int index
                                        width:2;height:3+2*(2-Math.abs(index-2));radius:1
                                        y:(7-height)/2
                                        color:root.theme.muted;opacity:.35
                                    }
                                }
                            }
                        }
                        Text {
                            id:footerMessage
                            anchors.fill:parent
                            visible:root.message!==''
                            text:root.message
                            color:root.message?root.theme.accent:root.theme.muted
                            font.family:root.settings.ui.font;font.pixelSize:11;elide:Text.ElideRight
                            verticalAlignment:Text.AlignVCenter
                        }
                    }
                }
            }
        }
    }
}
