import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell.Io

Pane {
    padding: 0
    implicitHeight: contents.implicitHeight
    background: Item {}
    palette.base: theme.background
    palette.button: theme.surface || theme.background
    palette.text: theme.text
    palette.buttonText: theme.text
    palette.highlight: theme.accent
    id: card
    required property var notice
    required property var theme
    required property double now
    required property var controller
    property bool historyMode: false
    property bool choosing: false
    property string error: ''
    function run(action, until) {
        if(card.historyMode || card.controller.locked || snooze.running)return;
        const argv=card.notice.data.todoCommand;
        if(!Array.isArray(argv) || argv.length!==2){card.error='Invalid todo command';return;}
        snooze.command=argv.concat([action,card.notice.data.taskId,card.notice.data.period]).concat(until?[until]:[]);
        snooze.running=true;
    }
    ColumnLayout {
        id: contents; width: parent.width
    Label {text:(notice.data.group || 'General')+' · Due '+(notice.data.due || 'today');color:theme.text}
    RowLayout {
        enabled:!card.historyMode && !card.controller.locked && !snooze.running
        Button {text:'Snooze';onClicked:card.choosing=!card.choosing}
        Button {text:'Done';onClicked:card.run('done','')}
        Button {text:'Dismiss';onClicked:card.run('dismiss','')}
    }
    SnoozePicker {
        visible:card.choosing;Layout.fillWidth:true;ink:theme.text;accent:theme.accent
        onChosen: until=>card.run('snooze',until)
    }
    Label {visible:!!card.error;text:card.error;color:theme.accent;wrapMode:Text.Wrap;Layout.fillWidth:true}
    }
    Process {id:snooze;onExited:(code,status)=>{if(code===0)card.controller.close(card.notice.id,false);else card.error='Could not save change; try again when Si is reachable.';}}
}
