import QtQuick
import Quickshell
import Quickshell.Io
import "../qml" as App
import "../qml/MorphData.js" as Data
ShellRoot {
    id:root
    property string symbol:'coffee'
    property bool sampling:false
    property var theme:({iconSet:'proicons',animateIcons:true,iconSize:64,fontSize:14,minItemSize:64,maxTextWidth:100,font:'sans-serif',iconFont:'Material Symbols Rounded',text:'#ffffff',background:'#26233a',surface:'#26233a',border:'#555169'})
    PanelWindow {
        id:win;anchors{top:true;left:true}implicitWidth:100;implicitHeight:100;color:'#26233a';exclusionMode:ExclusionMode.Ignore
        App.Part {id:part;visible:!root.sampling;x:18;y:18;kind:'icon';window:win;controller:root;theme:root.theme;moduleConfig:({});context:({});row:({id:'toggle',type:'icon',icon:{kind:'symbol',value:root.symbol},state:{}})}
        App.MorphIcon {id:sample;visible:root.sampling;x:18;y:18;width:64;height:64;symbol:'';animate:false}
    }
    IpcHandler {
        target:'morph'
        function frame(from:string,to:string,progress:real,path:string):void {
            root.sampling=true;
            sample.sourceShape=Data.shapes[from];sample.targetShape=Data.shapes[to];sample.progress=progress;sample.requestPaint();
            sample.grabToImage(result=>result.saveToFile(path));
        }
        function request(symbol:string,path:string):string {
            root.sampling=false;
            if(symbol)root.symbol=symbol;
            const m=part.children.find(c=>c.objectName==='toggleMorph');
            if(path)part.grabToImage(result=>result.saveToFile(path));
            return JSON.stringify({running:m.morphing,progress:m.progress,symbol:root.symbol});
        }
    }
}
