import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "MonitorGeometry.js" as Geometry

Column {
    id:details
    required property var popup
    readonly property var theme:popup.theme
    readonly property var info:popup.monitorPanel?popup.panelData:({})
    readonly property var live:info.monitors || []
    property var draft:[]
    property var basis:[]
    property string revision:''
    property string selected:''
    property bool dirty:false
    property bool mapDragging:false
    property string activePreview:''
    property var viewport:({x:0,y:0,width:1,height:1})
    readonly property bool busy:popup.feedback.busy===true || !!popup.controller.state.monitorChange
    readonly property var current:draft.find(m=>m.name===selected) || null
    readonly property bool editable:!!current?.connected && current?.configured===true
    readonly property bool usable:draft.some(m=>m.connected && m.settings.enabled && !m.settings.mirror)
    readonly property bool needsKeyboard:!!(positionEditors?.children.some(c=>c.activeFocus) || modeChoice?.popup.visible || scaleChoice?.popup.visible || rotationChoice?.popup.visible || vrrChoice?.popup.visible || colourChoice?.popup.visible || mirrorChoice?.popup.visible)
    spacing:12
    function reset(){
        basis=JSON.parse(JSON.stringify(live));draft=JSON.parse(JSON.stringify(live));revision=info.revision || '';dirty=false;
        if(!draft.some(m=>m.name===selected))selected=(draft.find(m=>m.enabled) || draft[0])?.name || '';
        viewport=Geometry.bounds(draft);
    }
    onLiveChanged:if(!dirty && !mapDragging && !needsKeyboard)reset()
    function edit(name,patch){
        draft=draft.map(m=>m.name===name?Object.assign({},m,{settings:Object.assign({},m.settings,patch)}):m);
        dirty=Geometry.changes(basis,draft).length>0;if(!mapDragging)viewport=Geometry.bounds(draft);
    }
    function unmirrorCopies(name){for(const m of draft)if(m.connected && m.settings.mirror===name)edit(m.name,{mirror:''});}
    function stageEnabled(name,enabled){if(!enabled)unmirrorCopies(name);edit(name,{enabled:enabled});}
    function stageTV(enabled){const tv=draft.find(m=>m.name===info.tv && m.connected);if(tv){selected=tv.name;unmirrorCopies(tv.name);edit(tv.name,{enabled:enabled,mirror:''});}}
    function stageMirror(){
        const tv=draft.find(m=>m.name===info.tv && m.connected),copy=draft.find(m=>m.name===info.mirrorOutput && m.connected);
        if(!tv || !copy)return;
        unmirrorCopies(copy.name);edit(tv.name,{enabled:true,mirror:''});edit(copy.name,{enabled:true,mirror:tv.name});selected=copy.name;
    }
    function apply(){popup.action('apply',JSON.stringify({revision:revision,edits:Geometry.changes(basis,draft)}));}
    function inspect(){return {selected:selected,dirty:dirty,monitors:draft,needsKeyboard:needsKeyboard,usable:usable,map:viewport};}
    Connections {
        target:popup.controller
        function onStateChanged(){const token=popup.controller.state.monitorChange?.token || '';if(token)details.activePreview=token;else if(details.activePreview){details.activePreview='';details.dirty=false;details.reset();}}
    }
    component Label: Text {font.family:details.theme.font;font.pixelSize:12;color:details.theme.text;textFormat:Text.PlainText}
    component Choice: ComboBox {
        id:control;required property var values;required property var selection
        function syncSelection(){currentIndex=values.findIndex(v=>v.value===selection);}
        onValuesChanged:Qt.callLater(syncSelection)
        onSelectionChanged:Qt.callLater(syncSelection)
        Component.onCompleted:syncSelection()
        model:values.map(v=>v.label);implicitHeight:34;enabled:details.editable && !details.busy
        font.family:details.theme.font;font.pixelSize:12
        background:Rectangle {radius:7;color:details.theme.surface;border.width:control.activeFocus?1:0;border.color:details.theme.accent}
        contentItem:Label {text:control.displayText;verticalAlignment:Text.AlignVCenter;leftPadding:10;rightPadding:24;elide:Text.ElideRight}
        indicator:Label {x:parent.width-20;y:9;text:'⌄';color:details.theme.muted}
        delegate:ItemDelegate {
            required property string modelData
            required property int index
            width:control.width;height:32
            background:Rectangle {color:parent.highlighted?details.theme.surface:details.theme.background}
            contentItem:Label {text:parent.modelData;elide:Text.ElideRight;verticalAlignment:Text.AlignVCenter}
            highlighted:control.highlightedIndex===index
        }
        popup:Popup {
            y:control.height+4;width:control.width;padding:4
            implicitHeight:Math.min(240,contentItem.implicitHeight+8)
            background:Rectangle {radius:8;color:details.theme.background;border.color:details.theme.border}
            contentItem:ListView {clip:true;implicitHeight:contentHeight;model:control.popup.visible?control.delegateModel:null;currentIndex:control.highlightedIndex;ScrollBar.vertical:ScrollBar {}}
        }
    }
    RowLayout {
        width:parent.width
        Label {text:'Display arrangement';Layout.fillWidth:true}
        PanelButton {objectName:'display-desktop';theme:details.theme;showLabel:true;text:'Desktop';enabled:!details.busy && details.draft.some(m=>m.name===details.info.tv && m.connected && m.configured);onClicked:details.stageTV(false)}
        PanelButton {objectName:'display-game';theme:details.theme;showLabel:true;text:'Game / TV';enabled:!details.busy && details.draft.some(m=>m.name===details.info.tv && m.connected && m.configured);onClicked:details.stageTV(true)}
        PanelButton {objectName:'display-tv-mirror';theme:details.theme;showLabel:true;text:'TV mirror';enabled:!details.busy && [details.info.tv,details.info.mirrorOutput].every(name=>details.draft.some(m=>m.name===name && m.connected && m.configured));onClicked:details.stageMirror()}
    }
    Rectangle {
        id:map;objectName:'display-map';width:parent.width;height:200;radius:10;color:details.theme.surface;clip:true
        readonly property bool panelControl:true
        readonly property var fit:Geometry.fit(details.viewport,width,height)
        Repeater {
            model:details.draft
            delegate:Rectangle {
                id:monitor;required property var modelData;required property int index
                objectName:'display-map-'+modelData.name
                readonly property var dimensions:Geometry.size(modelData)
                readonly property var copies:Geometry.mirrors(modelData,details.draft)
                readonly property bool selected:details.selected===modelData.name || copies.some(m=>m.name===details.selected)
                visible:!Geometry.mirrorSource(modelData,details.draft)
                property real previewX:modelData.settings.x
                property real previewY:modelData.settings.y
                x:map.fit.x+(dragInput.pressed?previewX:modelData.settings.x)*map.fit.scale;y:map.fit.y+(dragInput.pressed?previewY:modelData.settings.y)*map.fit.scale
                width:Math.max(18,dimensions.width*map.fit.scale);height:Math.max(18,dimensions.height*map.fit.scale)
                radius:4;color:monitor.selected?Qt.tint(details.theme.background,Qt.rgba(1,.33,.65,.14)):details.theme.background
                border.width:monitor.selected?2:1;border.color:monitor.selected?details.theme.accent:details.theme.border
                opacity:modelData.connected?(modelData.settings.enabled?1:.65):.4
                Column {
                    anchors.centerIn:parent;width:parent.width-8;spacing:3
                    Label {width:parent.width;text:String(monitor.index+1)+' · '+monitor.modelData.name;horizontalAlignment:Text.AlignHCenter;font.pixelSize:11;elide:Text.ElideRight}
                    Label {width:parent.width;visible:monitor.copies.length>0;text:'+ '+monitor.copies.map(m=>m.name).join(', ');horizontalAlignment:Text.AlignHCenter;font.pixelSize:10;elide:Text.ElideRight}
                    Label {width:parent.width;text:!monitor.modelData.connected?'Disconnected':monitor.copies.length?'Mirrored':monitor.modelData.settings.enabled?'On':'Off';horizontalAlignment:Text.AlignHCenter;font.pixelSize:10;color:details.theme.muted;elide:Text.ElideRight}
                }
                MouseArea {
                    id:dragInput;anchors.fill:parent;acceptedButtons:Qt.LeftButton;preventStealing:true;cursorShape:monitor.modelData.connected && monitor.modelData.configured && !details.busy?Qt.OpenHandCursor:Qt.PointingHandCursor
                    property point origin;property point start
                    onPressed:mouse=>{details.selected=monitor.modelData.name;origin=Qt.point(monitor.modelData.settings.x,monitor.modelData.settings.y);monitor.previewX=origin.x;monitor.previewY=origin.y;start=mapToItem(map,mouse.x,mouse.y);details.mapDragging=true;}
                    onPositionChanged:mouse=>{
                        if(!pressed || !monitor.modelData.connected || !monitor.modelData.configured || details.busy)return;
                        const point=mapToItem(map,mouse.x,mouse.y),x=origin.x+(point.x-start.x)/map.fit.scale,y=origin.y+(point.y-start.y)/map.fit.scale;
                        const snap=Geometry.snap(monitor.modelData,details.draft,x,y,10/map.fit.scale);
                        monitor.previewX=snap.x;monitor.previewY=snap.y;
                    }
                    onReleased:{details.mapDragging=false;if(monitor.modelData.connected && monitor.modelData.configured && !details.busy)details.edit(monitor.modelData.name,{x:monitor.previewX,y:monitor.previewY});}
                    onCanceled:details.mapDragging=false
                }
            }
        }
    }
    Label {width:parent.width;text:'Select a display · drag to arrange · sizes reflect resolution and scaling';wrapMode:Text.Wrap;font.pixelSize:10;color:details.theme.muted}
    Row {
        width:parent.width;spacing:6
        Repeater {
            model:details.draft
            delegate:PanelButton {
                required property var modelData
                objectName:'display-select-'+modelData.name
                theme:details.theme;showLabel:true;text:modelData.name;accented:details.selected===modelData.name
                onClicked:details.selected=modelData.name
            }
        }
    }
    Column {
        width:parent.width;spacing:10;visible:!!details.current
        RowLayout {
            width:parent.width
            Column {Layout.fillWidth:true;spacing:3
                Label {width:parent.width;text:details.current?.label || '';font.pixelSize:14;elide:Text.ElideRight}
                Label {text:details.current?.connected?(details.current.configured?'Connected · ':'Set up this display in Hyprland first · ')+(details.current.settings.mode || '').replace('@',' · ')+' Hz':'Disconnected · last configured position';font.pixelSize:10;color:details.theme.muted}
            }
            Switch {
                id:enableSwitch;objectName:'display-enabled';checked:details.current?.settings.enabled===true
                enabled:details.editable && !details.busy;focusPolicy:Qt.NoFocus
                onClicked:{details.stageEnabled(details.selected,checked);checked=Qt.binding(()=>details.current?.settings.enabled===true);}
                indicator:Rectangle {implicitWidth:38;implicitHeight:22;radius:11;color:enableSwitch.checked?details.theme.accent:details.theme.surface;border.color:details.theme.border;border.width:enableSwitch.checked?0:1
                    Rectangle {x:enableSwitch.checked?18:3;y:3;width:16;height:16;radius:8;color:enableSwitch.checked?details.theme.background:details.theme.text}}
            }
        }
        GridLayout {
            width:parent.width;columns:2;columnSpacing:12;rowSpacing:8
            Label {text:'Use as'}
            Choice {id:mirrorChoice;objectName:'display-mirror';Layout.fillWidth:true
                values:[{value:'',label:'Extend desktop'}].concat(details.draft.filter(m=>m.name!==details.selected && m.connected && m.settings.enabled && !m.settings.mirror && !details.draft.some(p=>p.connected && p.settings.enabled && p.settings.mirror===details.selected)).map(m=>({value:m.name,label:'Mirror '+m.name})))
                selection:details.current?.settings.mirror || ''
                onActivated:index=>details.edit(details.selected,{mirror:values[index].value})}
            Label {text:'Resolution / refresh'}
            Choice {id:modeChoice;objectName:'display-mode';Layout.fillWidth:true
                values:(details.current?.availableModes || []).map(m=>({value:m,label:m.replace('@',' · ')+' Hz'}))
                selection:details.current?.settings.mode
                onActivated:index=>{const mode=values[index].value;details.edit(details.selected,{mode:mode});if(!Geometry.scales(details.current).includes(details.current.settings.scale))details.edit(details.selected,{scale:1});}}
            Label {text:'Scale'}
            Choice {id:scaleChoice;objectName:'display-scale';Layout.fillWidth:true
                values:details.current?Geometry.scales(details.current).map(v=>({value:v,label:Math.round(v*100)+'%'})):[]
                selection:details.current?.settings.scale
                onActivated:index=>details.edit(details.selected,{scale:values[index].value})}
            Label {text:'Rotation'}
            Choice {id:rotationChoice;objectName:'display-rotation';Layout.fillWidth:true
                values:[{value:0,label:'Landscape'},{value:1,label:'Portrait · 90°'},{value:2,label:'Upside down · 180°'},{value:3,label:'Portrait · 270°'}]
                selection:details.current?.settings.transform
                onActivated:index=>details.edit(details.selected,{transform:values[index].value})}
            Label {text:details.current?.settings.mirror?'Extend X / Y':'Position · X / Y'}
            RowLayout {
                id:positionEditors;Layout.fillWidth:true;spacing:6
                Repeater {
                    model:['x','y']
                    delegate:TextField {
                        required property string modelData
                        objectName:'display-position-'+modelData;Layout.fillWidth:true;implicitWidth:80;implicitHeight:32
                        text:String(details.current?.settings[modelData] || 0);enabled:details.editable && !details.busy && !details.current?.settings.mirror
                        color:details.theme.text;font.family:details.theme.font;font.pixelSize:12;selectByMouse:true
                        validator:IntValidator {bottom:-32768;top:32768}
                        background:Rectangle {radius:7;color:details.theme.surface;border.width:parent.activeFocus?1:0;border.color:details.theme.accent}
                        onEditingFinished:if(acceptableInput){const p={};p[modelData]=Number(text);details.edit(details.selected,p);}
                    }
                }
            }
            Label {text:'Adaptive sync';visible:!!details.current?.hardwareDetails?.vrrCapable}
            Choice {id:vrrChoice;objectName:'display-vrr';Layout.fillWidth:true;visible:!!details.current?.hardwareDetails?.vrrCapable
                values:[{value:-1,label:'Use Hyprland setting'},{value:0,label:'Off'},{value:2,label:'Fullscreen only'},{value:1,label:'Always'}]
                selection:details.current?.settings.vrr
                onActivated:index=>details.edit(details.selected,{vrr:values[index].value})}
            Label {text:'Colour mode';visible:!!details.current?.hardwareDetails?.hdr}
            Choice {id:colourChoice;objectName:'display-colour';Layout.fillWidth:true;visible:!!details.current?.hardwareDetails?.hdr
                values:[{value:'auto',label:'Automatic'},{value:'srgb',label:'Standard · sRGB'},{value:'hdr',label:'HDR'}]
                selection:details.current?.settings.cm
                onActivated:index=>details.edit(details.selected,{cm:values[index].value})}
        }
        Label {width:parent.width;wrapMode:Text.Wrap;visible:!!details.current?.settings.mirror;font.pixelSize:10;color:details.theme.muted
            text:'Shows '+(details.current?.settings.mirror || '')+' at this display’s resolution. Extend restores its independent position.'}
    }
    Label {width:parent.width;wrapMode:Text.Wrap;visible:!!text;color:details.theme.accent
        text:!details.usable?'Keep at least one connected display on':details.popup.feedback.error?details.popup.feedback.message:''}
    RowLayout {
        width:parent.width
        Label {Layout.fillWidth:true;text:details.busy?'Applying…':details.dirty?'Changes not applied':'Current Hyprland layout';font.pixelSize:11;color:details.theme.muted}
        PanelButton {objectName:'display-reset';theme:details.theme;showLabel:true;text:'Reset';enabled:details.dirty && !details.busy;onClicked:details.reset()}
        PanelButton {objectName:'display-apply';theme:details.theme;showLabel:true;text:'Apply';enabled:details.dirty && details.usable && !details.busy;onClicked:details.apply()}
    }
}
