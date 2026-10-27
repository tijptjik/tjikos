import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    id:root
    readonly property bool ready:true
    ShowcaseStage {
        id:stage;controller:root
        toolId:Quickshell.env('SHOWCASE_TOOL')
        toolName:Quickshell.env('SHOWCASE_NAME')
        introHeading:Quickshell.env('SHOWCASE_HEADING')
        introCaption:Quickshell.env('SHOWCASE_CAPTION')
        repository:'github.com/tijptjik/tjikos/tree/main/apps/'+toolId
        iconRoot:Quickshell.env('SHOWCASE_ICONS')
        coverWindows:true
    }
    IpcHandler {
        target:'showcase'
        function request(data:string):string {
            const value=JSON.parse(data);stage.scene(value);
            return JSON.stringify({ready:true,mode:stage.mode,chapter:stage.chapter});
        }
    }
}
