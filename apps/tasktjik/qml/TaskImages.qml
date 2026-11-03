import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io

Rectangle {
    id: panel
    signal backgroundClicked()
    property var theme
    property var task: null
    property var sources: ({})
    property var failures: ({})
    property string shownTask: ''
    property string backend: Quickshell.env('TASKTJIK_HOME') ? Quickshell.env('TASKTJIK_HOME')+'/tasktjik-api' : 'tasktjik'
    readonly property var images: task ? (task.images || []) : []
    color:theme.background
    radius:10
    onTaskChanged: {
        const id=task?task.id:'';
        if(id!==shownTask) {
            shownTask=id;
            scroll.contentItem.contentY=0;
            const keep={};for(const image of images)if(sources[image.id])keep[image.id]=sources[image.id];
            sources=keep;
        }
        Qt.callLater(loadNext);
    }
    function loadNext() {
        if(loader.running || !task)return;
        const image=images.find(i=>!sources[i.id] && !failures[i.id]);
        if(!image)return;
        loader.imageId=image.id;loader.taskId=task.id;
        loader.output='';loader.failure='';loader.running=true;
    }
    Process {
        id:loader
        property string imageId
        property string taskId
        property string output
        property string failure
        command:[panel.backend,'request']
        stdinEnabled:true
        onStarted:{write(JSON.stringify({action:'image-get',id:taskId,image_id:imageId}));stdinEnabled=false;}
        stdout:StdioCollector {onStreamFinished:loader.output=text}
        stderr:StdioCollector {onStreamFinished:loader.failure=text.trim()}
        onExited:(code,status)=>{
            if(code===0) {
                try {panel.sources=Object.assign({},panel.sources,{[imageId]:JSON.parse(output).source});}
                catch(e){panel.failures=Object.assign({},panel.failures,{[imageId]:'Could not load image'});}
            } else panel.failures=Object.assign({},panel.failures,{[imageId]:failure || 'Could not load image'});
            // Retain only the current task's decoded sources in the view.
            const keep={};for(const image of panel.images)if(panel.sources[image.id])keep[image.id]=panel.sources[image.id];
            panel.sources=keep;
            stdinEnabled=true;Qt.callLater(panel.loadNext);
        }
    }
    ColumnLayout {
        anchors.fill:parent;anchors.margins:16;spacing:12
        Label {text:'Images · '+panel.images.length;color:theme.muted;font.pixelSize:12}
        Label {text:panel.task?panel.task.title:'';textFormat:Text.PlainText;color:theme.text;font.pixelSize:16;font.weight:Font.DemiBold;Layout.fillWidth:true;wrapMode:Text.Wrap;maximumLineCount:2;elide:Text.ElideRight}
        KineticScrollView {
            id:scroll
            onBackgroundClicked:panel.backgroundClicked()
            Layout.fillWidth:true;Layout.fillHeight:true;clip:true
            contentWidth:availableWidth
            ScrollBar.horizontal.policy:ScrollBar.AlwaysOff
            Column {
                width:scroll.availableWidth;spacing:16
                Repeater {
                    model:panel.images
                    Column {
                        required property var modelData
                        width:parent.width;spacing:8
                        Image {
                            objectName:'task-image-'+modelData.id
                            width:parent.width
                            height:Math.max(80,Math.min(scroll.availableHeight,width*modelData.height/modelData.width))
                            source:panel.sources[modelData.id] || ''
                            sourceSize.width:Math.min(1600,width*2)
                            fillMode:Image.PreserveAspectFit
                            asynchronous:true
                            Accessible.name:'Task image'
                            Label {anchors.centerIn:parent;visible:parent.status!==Image.Ready;text:panel.failures[modelData.id] || (parent.status===Image.Error?'Could not render image':'Loading image…');width:parent.width;color:theme.muted;wrapMode:Text.Wrap;horizontalAlignment:Text.AlignHCenter}
                        }
                        Button {visible:!!panel.failures[modelData.id];text:'Retry image';onClicked:{const next=Object.assign({},panel.failures);delete next[modelData.id];panel.failures=next;panel.loadNext();}}
                    }
                }
            }
        }
        Label {text:'Ctrl+V to add another image';color:theme.muted;font.pixelSize:11}
    }
}
