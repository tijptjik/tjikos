import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: picker
    property color ink: '#e0def4'
    property color accent: '#ff53a6'
    property bool calendarOpen: false
    property date month: new Date(new Date().getFullYear(), new Date().getMonth(), 1)
    signal chosen(string until)
    function choose(label) {
        let d=new Date();
        const hours={Afternoon:14,Evening:18,Night:21};
        if(hours[label]) {d.setHours(hours[label],0,0,0);if(d<=new Date())d.setDate(d.getDate()+1);}
        else {d.setHours(9,0,0,0);const weekday=d.getDay();d.setDate(d.getDate()+(label==='Tomorrow'?1:label==='Weekend'?((6-weekday+7)%7 || 7):((8-weekday)%7 || 7)));}
        chosen(d.toISOString());
    }
    Flow {
        Layout.fillWidth:true; Layout.minimumWidth:0; Layout.preferredWidth:1; Layout.preferredHeight:childrenRect.height; spacing:4
        Repeater {
            model:['Afternoon','Evening','Night','Tomorrow','Weekend','Next Week','Later']
            Button {required property string modelData;text:modelData;onClicked: modelData==='Later'?picker.calendarOpen=true:picker.choose(modelData)}
        }
    }
    ColumnLayout {
        visible:picker.calendarOpen;Layout.fillWidth:true
        RowLayout {
            Button {text:'‹';onClicked:picker.month=new Date(picker.month.getFullYear(),picker.month.getMonth()-1,1)}
            Label {Layout.fillWidth:true;text:Qt.formatDate(picker.month,'MMMM yyyy');color:picker.ink;horizontalAlignment:Text.AlignHCenter}
            Button {text:'›';onClicked:picker.month=new Date(picker.month.getFullYear(),picker.month.getMonth()+1,1)}
        }
        GridLayout {
            columns:7;Layout.fillWidth:true;columnSpacing:2;rowSpacing:2
            Repeater {model:['Mon','Tue','Wed','Thu','Fri','Sat','Sun'];Label {required property string modelData;required property int index;text:modelData;color:index>=5?picker.accent:picker.ink;Layout.fillWidth:true;horizontalAlignment:Text.AlignHCenter}}
            Repeater {
                model:42
                Button {
                    required property int index
                    readonly property date dateValue:new Date(picker.month.getFullYear(),picker.month.getMonth(),index-((picker.month.getDay()+6)%7)+1,9)
                    Layout.fillWidth:true;Layout.minimumWidth:0;Layout.preferredWidth:32;implicitHeight:30
                    text:dateValue.getMonth()===picker.month.getMonth()?dateValue.getDate():''
                    enabled:text!=='' && dateValue>new Date()
                    contentItem:Text {text:parent.text;color:index%7>=5?picker.accent:picker.ink;opacity:parent.enabled?1:.3;horizontalAlignment:Text.AlignHCenter;verticalAlignment:Text.AlignVCenter}
                    onClicked:picker.chosen(dateValue.toISOString())
                }
            }
        }
    }
}
