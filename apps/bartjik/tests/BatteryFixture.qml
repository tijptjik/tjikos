import QtQuick
import Quickshell
import Quickshell.Io
import "../qml" as App
ShellRoot {
    id:root
    property var battery:({percent:67,external:false})
    property var theme:({iconSet:'proicons',animateIcons:true,greyscaleIcons:true,iconSize:18,fontSize:14,minItemSize:20,maxTextWidth:100,font:'sans-serif',iconFont:'Material Symbols Rounded',text:'#e5e5e5',muted:'#a5a0ba',background:'#26233a',surface:'#26233a',border:'#555169'})
    PanelWindow {
        id:win;anchors{top:true;left:true}implicitWidth:220;implicitHeight:120;color:'#26233a';exclusionMode:ExclusionMode.Ignore
        Rectangle {
        id:stage;anchors.fill:parent;color:'#26233a'
        App.BatteryIcon {id:large;x:18;y:18;width:64;height:64;battery:root.battery;theme:root.theme}
        Row {
            x:110;y:40;spacing:14
            App.Part {id:part;kind:'icon';window:win;controller:root;theme:root.theme;moduleConfig:({provider:'battery'});context:({});row:({id:'battery',type:'icon',icon:{kind:'symbol',value:root.battery.external?'bolt':'battery_grid'},battery:root.battery,state:{}})}
            App.Part {kind:'icon';window:win;controller:root;theme:root.theme;moduleConfig:({});context:({});row:({id:'desktop',type:'icon',icon:{kind:'symbol',value:'desktop_windows'},state:{}})}
        }
        }
    }
    IpcHandler {
        target:'battery'
        function state(percent:real,external:bool,path:string):string {
            root.battery={percent:percent,external:external};
            if(path)stage.grabToImage(result=>result.saveToFile(path));
            return JSON.stringify({count:large.count,tone:large.tone,progress:large.progress,running:large.morphing});
        }
    }
}
