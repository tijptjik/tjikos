import QtQuick
import "Logic.js" as Logic

Item {
    id: moduleView
    required property var controller
    required property var bar
    required property string moduleId
    required property var theme
    required property var window
    required property bool horizontal
    readonly property var moduleConfig: bar.moduleConfigs[moduleId] || {}
    readonly property var entries: controller.displayItems(bar,moduleId).filter(i=>i.visible!==false)
    readonly property string tailnetModule: moduleConfig.provider==='mullvad'?controller.connectivityPeer(bar,moduleId,'tailscale'):''
    readonly property bool tailnetOn: !!tailnetModule && controller.items(bar,tailnetModule)[0]?.panel?.connection==='Running'
    readonly property bool spacedWorkspaces: moduleConfig.provider==='workspaces' && moduleConfig.appearance==='separators'
    readonly property bool ringWorkspaces: moduleConfig.provider==='workspaces' && moduleConfig.appearance==='ring'
    function updateRing() {
        if(!ringWorkspaces){activeRing.visible=false;return;}
        const active=entries.find(e=>e.state?.active || e.state?.focused);
        const target=active && grid.children.find(e=>e.key===active.id);
        if(!target){activeRing.visible=false;return;}
        const point=target.mapToItem(moduleView,target.width/2,target.height/2);
        activeRing.x=point.x-activeRing.width/2;
        activeRing.y=point.y-activeRing.height/2;
        activeRing.visible=true;
        Qt.callLater(()=>{activeRing.initialized=true;});
    }
    // Equal visible gutters around each dot; multi-digit labels take their natural width.
    readonly property real workspaceDotSize: 4
    readonly property real workspaceDotGap: 6
    function previewTargets(scope) {
        return grid.children.filter(e=>e.key!==undefined).map(e=>{
            const p=e.mapToItem(scope,0,0),m=moduleConfig;
            return {x:p.x,y:p.y,width:e.width,height:e.height,row:e.row,provider:m.provider,
                enabled:m.provider==='workspaces'?m.preview?.enabled===true:Logic.detailHover(m,e.row),
                options:m.provider==='workspaces'?m.preview:m.panel,
                context:{barKey:bar.key,bar:bar.id,monitor:bar.screen,module:moduleId,item:e.key,part:'item'}};
        });
    }
    visible: entries.length>0
    implicitWidth: grid.implicitWidth
    implicitHeight: grid.implicitHeight
    onEntriesChanged: {Logic.sync(keys,entries.map(i=>i.id));Qt.callLater(updateRing);}
    Component.onCompleted: {Logic.sync(keys,entries.map(i=>i.id));Qt.callLater(updateRing);}
    onRingWorkspacesChanged: Qt.callLater(updateRing)
    ListModel {id: keys}
    Grid {
        id: grid
        onPositioningComplete: moduleView.updateRing()
        verticalItemAlignment: Grid.AlignVCenter
        columns: moduleView.horizontal ? Math.max(1,keys.count) : 1
        columnSpacing: moduleView.spacedWorkspaces ? 0 : (moduleView.moduleConfig.style?.itemGap ?? moduleView.theme.itemGap)
        rowSpacing: moduleView.theme.itemGap
        Repeater {
            model: keys
            delegate: Item {
                id: entry
                required property string key
                required property int index
                readonly property var row: moduleView.entries.find(i=>i.id===key) || {id:key,type:'text',text:{value:''}}
                readonly property bool media: moduleView.moduleConfig.provider==='playerctl' && !row.idle
                readonly property bool separator: moduleView.spacedWorkspaces && index>0
                readonly property bool chip: moduleView.moduleConfig.provider==='workspaces' && moduleView.moduleConfig.appearance==='chips'
                readonly property bool compactWorkspace: chip || moduleView.ringWorkspaces
                implicitWidth: moduleView.spacedWorkspaces ? workspaceLabel.naturalTextWidth+2*moduleView.workspaceDotGap+moduleView.workspaceDotSize : compactWorkspace ? Math.max(32,workspaceLabel.naturalTextWidth+14) : parts.implicitWidth
                implicitHeight: compactWorkspace ? 24 : parts.implicitHeight
                Rectangle {anchors.fill:parent;radius:6;visible:entry.chip;color:(entry.row.state?.active || entry.row.state?.focused)?Qt.rgba(1,1,1,.10):workspaceLabel.hovered?Qt.rgba(1,1,1,.04):'transparent'
                    Behavior on color {ColorAnimation {duration:180}}}
                Row {
                id:parts;anchors.verticalCenter:parent.verticalCenter
                x:(entry.width-width)/2
                spacing: moduleView.moduleConfig.style?.iconTextGap ?? moduleView.theme.iconTextGap
                Part {
                    id:entryIcon
                    kind: 'icon'; visible: entry.row.type!=='text'
                    row: entry.row; theme: moduleView.theme; moduleConfig: moduleView.moduleConfig
                    controller: moduleView.controller; window: moduleView.window
                    context: ({barKey:moduleView.bar.key,bar:moduleView.bar.id,monitor:moduleView.bar.screen,module:moduleView.moduleId})
                }
                Part {
                    id:workspaceLabel
                    kind: 'text'; visible: !entry.media && entry.row.type!=='icon'
                    width: moduleView.spacedWorkspaces || entry.compactWorkspace ? entry.width : implicitWidth
                    height: entry.compactWorkspace ? entry.height : implicitHeight
                    opacity: entry.compactWorkspace && !workspaceLabel.hovered && !entry.row.state?.active && !entry.row.state?.focused && !entry.row.state?.urgent ? .65 : 1
                    row: entry.row; theme: moduleView.theme; moduleConfig: moduleView.moduleConfig
                    controller: moduleView.controller; window: moduleView.window
                    context: ({barKey:moduleView.bar.key,bar:moduleView.bar.id,monitor:moduleView.bar.screen,module:moduleView.moduleId})
                }
                Part {
                    id:mediaArtist
                    kind:'artist';visible:entry.media && !!entry.row.artist?.value
                    row:entry.row;theme:moduleView.theme;moduleConfig:moduleView.moduleConfig
                    controller:moduleView.controller;window:moduleView.window
                    context:({barKey:moduleView.bar.key,bar:moduleView.bar.id,monitor:moduleView.bar.screen,module:moduleView.moduleId})
                }
                Text {
                    visible:mediaArtist.visible
                    text:'·';color:moduleView.theme.muted
                    font.family:mediaArtist.style.font;font.pixelSize:mediaArtist.style.fontSize
                    // A fallback font's line metrics must not enlarge the media island.
                    height:mediaArtist.height;verticalAlignment:Text.AlignVCenter
                    anchors.verticalCenter:parent.verticalCenter
                }
                Part {
                    kind:'track';visible:entry.media
                    row:entry.row;theme:moduleView.theme;moduleConfig:moduleView.moduleConfig
                    controller:moduleView.controller;window:moduleView.window
                    context:({barKey:moduleView.bar.key,bar:moduleView.bar.id,monitor:moduleView.bar.screen,module:moduleView.moduleId})
                }
                }
                Row {
                    objectName:'tailscale-on-indicator'
                    visible:moduleView.tailnetOn
                    width:entryIcon.style.iconSize;height:3;spacing:Math.max(0,(width-9)/2)
                    x:parts.x+entryIcon.x+(entryIcon.width-width)/2
                    anchors.bottom:parent.bottom;anchors.bottomMargin:-3
                    Repeater {
                        model:3
                        Rectangle {width:3;height:3;radius:1.5;color:moduleView.theme.muted}
                    }
                }
                Rectangle {objectName:'workspace-separator' ;visible:entry.separator;width:moduleView.workspaceDotSize;height:width;radius:width/2;x:-width/2;y:Math.round((parent.height-height)/2);color:moduleView.theme.muted;opacity:.7}
            }
        }
    }
    Rectangle {
        id:activeRing;objectName:'workspace-active-ring'
        property bool initialized:false
        width:22;height:22;radius:11;visible:false;z:1
        color:'transparent';border.width:1.5;border.color:moduleView.theme.accent
        Behavior on x {enabled:activeRing.initialized;NumberAnimation {duration:240;easing.type:Easing.OutCubic}}
        Behavior on y {enabled:activeRing.initialized;NumberAnimation {duration:240;easing.type:Easing.OutCubic}}
    }

}
