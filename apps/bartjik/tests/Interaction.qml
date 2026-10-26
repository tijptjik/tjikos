import QtQuick
import QtTest
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import "../qml" as App

ShellRoot {
    id:root
    property var events: []
    property var theme: ({font:'monospace',iconFont:'Material Symbols Rounded',fontSize:14,iconSize:18,minItemSize:24,maxTextWidth:200,text:'#eeeeee',muted:'#aaaaaa',accent:'#ff53a6',surface:'#222222',border:'#555555'})
    function act(action,context,window,row) {events=events.concat([{action:action,context:context}]);}
    PanelWindow {
        id:win;anchors{top:true;left:true}
        implicitWidth:240;implicitHeight:160;color:'#222222';exclusionMode:ExclusionMode.Ignore
        WlrLayershell.namespace:'bartjik-interaction-fixture'
        Row {
            x:20;y:20
            App.Part {
                id:icon;kind:'icon';controller:root;window:win;theme:root.theme
                row:({id:'coin',type:'icon_text',icon:{kind:'glyph',value:'₿'},text:{value:'123'},tooltip:''})
                moduleConfig:({hoverDelay:25,doubleClickInterval:150,icon:{actions:{leftClick:['icon'],rightClick:['right'],middleClick:['middle'],doubleClick:['double'],hoverEnter:['enter'],hoverLeave:['leave'],scrollUp:['up'],scrollDown:['down']}}})
                context:({bar:'fixture',barKey:'DP-2/fixture',monitor:'DP-2',module:'prices'})
            }
            App.Part {
                id:text;kind:'text';controller:root;window:win;theme:root.theme;row:icon.row
                moduleConfig:({text:{actions:{leftClick:['text']}}});context:icon.context
            }
        }
    }
    // Exercise the real island hover layer as well as standalone parts.
    function items(bar,id) {return [{id:'toggle',type:'icon',icon:{kind:'glyph',value:'X'},tooltip:'Notifications : ON'}];}
    function findStatePart(item) {
        if(item.kind==='icon' && item.context?.module==='state')return item;
        for(const child of item.children || []) {const found=findStatePart(child);if(found)return found;}
        return null;
    }
    App.Island {
        id:stateIsland;parent:win.contentItem;x:20;y:100
        definition:({id:'controls',modules:['state']})
        bar:({id:'fixture',key:'DP-2/fixture',screen:'DP-2',thickness:44,moduleConfigs:{state:{provider:'DND',hoverDelay:25}},style:Object.assign({},root.theme,{padding:8,moduleGap:12,itemGap:6,iconTextGap:6,radius:8,background:'#222222',borderWidth:0})})
        horizontal:true;controller:root;window:win
    }
    TestEvent {id:input}
    IpcHandler {
        target:'interaction'
        function event(kind:string):string {
            if(kind==='debug')return JSON.stringify([{hovered:icon.hovered,entered:icon.hoverEntered}]);
            if(kind==='tooltip-enter') {const part=root.findStatePart(stateIsland);input.mouseMove(part,12,12,0,Qt.NoButton,Qt.NoModifier);}
            if(kind==='tooltip-debug') {
                const part=root.findStatePart(stateIsland),popup=part.data.find(o=>o.objectName==='part-tooltip');
                return JSON.stringify([{hovered:part.hovered,visible:popup.visible,text:popup.text}]);
            }
            if(kind==='reset')root.events=[];
            else if(kind==='left')input.mouseClick(icon,12,12,Qt.LeftButton,Qt.NoModifier,0);
            else if(kind==='right')input.mouseClick(icon,12,12,Qt.RightButton,Qt.NoModifier,0);
            else if(kind==='middle')input.mouseClick(icon,12,12,Qt.MiddleButton,Qt.NoModifier,0);
            else if(kind==='double')input.mouseDoubleClickSequence(icon,12,12,Qt.LeftButton,Qt.NoModifier,0);
            else if(kind==='text')input.mouseClick(text,12,12,Qt.LeftButton,Qt.NoModifier,0);
            else if(kind==='up')input.mouseWheel(icon,12,12,Qt.NoButton,Qt.NoModifier,0,120,0);
            else if(kind==='down')input.mouseWheel(icon,12,12,Qt.NoButton,Qt.NoModifier,0,-120,0);
            else if(kind==='enter')input.mouseMove(icon,12,12,0,Qt.NoButton,Qt.NoModifier);
            else if(kind==='move')input.mouseMove(icon,13,12,0,Qt.NoButton,Qt.NoModifier);
            else if(kind==='leave') {input.mouseMove(icon,180,50,0,Qt.NoButton,Qt.NoModifier);input.mouseClick(win.contentItem,200,70,Qt.LeftButton,Qt.NoModifier,0);}
            return JSON.stringify(root.events);
        }
    }
}
