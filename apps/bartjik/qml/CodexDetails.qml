import QtQuick
import QtQuick.Controls
import "CodexTimeline.js" as Timeline

Column {
    id:details
    required property var popup
    readonly property var theme:popup.theme
    readonly property var fields:popup.fields
    readonly property var accounts:fields.filter(f=>f.group==='account' || typeof f.progress==='number')
    // Rose Punk: rose overlap, blue deficit, bright green/teal surplus.
    readonly property color overlapColor:'#eb6f92'
    readonly property color timeOnlyColor:'#7188ff'
    readonly property color creditOnlyColor:'#2f87ab'
    property real now:Date.now()/1000
    function field(id){return fields.find(f=>f.id===id) || {};}
    function compact(value) {
        if(typeof value!=='number' || !isFinite(value))return '—';
        const n=Math.abs(value);
        return n>=1e9?(value/1e9).toFixed(1)+'B':n>=1e6?(value/1e6).toFixed(1)+'M':n>=1e3?(value/1e3).toFixed(1)+'k':String(value);
    }
    spacing:16
    Timer {interval:60000;repeat:true;running:details.visible && details.popup.opened;triggeredOnStart:true;onTriggered:details.now=Date.now()/1000}
    Item {
        width:parent.width;height:details.popup.row?.state?.stale?36:16
        Text {text:'WEEKLY ACCOUNTS';font.pixelSize:11;font.letterSpacing:1;color:details.theme.muted}
        Text {y:22;visible:details.popup.row?.state?.stale || false;text:'Cached · offline';font.pixelSize:11;color:details.theme.muted}
    }
    Repeater {
        model:details.accounts
        Item {
            id:account
            required property var modelData
            readonly property var countdown:Timeline.countdown(modelData.resetAt,details.now)
            readonly property real trackWidth:Math.max(0,width-96)
            width:details.width;height:52
            Text {id:accountName;y:1;width:Math.min(implicitWidth,Math.max(0,parent.width-plan.width-usage.width-24));text:account.modelData.label;font.family:details.theme.font;font.pixelSize:15;font.weight:Font.DemiBold;color:details.theme.text;textFormat:Text.PlainText;elide:Text.ElideRight}
            Text {id:plan;x:accountName.width+4;y:-4;text:account.modelData.plan || '';font.pixelSize:10;color:details.theme.muted;textFormat:Text.PlainText}
            Text {id:usage;anchors.right:parent.right;y:1;text:Timeline.remainingLabel(account.modelData.progress);font.family:details.theme.font;font.pixelSize:15;color:details.theme.text}
            Text {anchors.right:parent.right;y:27;text:account.countdown.label;font.family:details.theme.font;font.pixelSize:14;color:details.theme.text;Accessible.name:'Reset '+(account.countdown.hours>0?'in ':'')+text}
            Row {
                y:32;width:account.trackWidth;height:10;spacing:6
                Repeater {
                    model:7
                    Rectangle {
                        id:day
                        required property int index
                        readonly property var portions:Timeline.comparison(account.countdown.known?account.countdown.hours:null,account.modelData.progress,index)
                        width:Math.max(0,(account.trackWidth-36)/7);height:10;radius:2
                        color:portions.known?'black':details.theme.surface
                        Rectangle {
                            visible:day.portions.overlap>0
                            width:parent.width*day.portions.overlap;height:parent.height
                            radius:Math.min(2,width/2)
                            topRightRadius:day.portions.timeOnly>0 || day.portions.creditOnly>0?0:radius
                            bottomRightRadius:topRightRadius
                            color:details.overlapColor
                        }
                        Rectangle {
                            visible:day.portions.timeOnly>0
                            x:parent.width*day.portions.overlap;width:parent.width*day.portions.timeOnly;height:parent.height
                            radius:Math.min(2,width/2)
                            topLeftRadius:day.portions.overlap>0?0:radius;bottomLeftRadius:topLeftRadius
                            color:details.timeOnlyColor
                        }
                        Rectangle {
                            visible:day.portions.creditOnly>0
                            x:parent.width*day.portions.overlap;width:parent.width*day.portions.creditOnly;height:parent.height
                            radius:Math.min(2,width/2)
                            topLeftRadius:day.portions.overlap>0?0:radius;bottomLeftRadius:topLeftRadius
                            color:details.creditOnlyColor
                        }
                    }
                }
            }
        }
    }
    Column {
        width:parent.width;spacing:12
        Rectangle {width:parent.width;height:1;color:details.theme.surface}
        Item {
            width:parent.width;height:16
            Text {text:'LAST 24 HOURS';font.pixelSize:11;font.letterSpacing:1;color:details.theme.muted}
            Text {anchors.right:parent.right;text:details.field('requests').failures>0?details.field('requests').failures+' failed requests':'';font.pixelSize:11;color:details.theme.muted}
        }
        Grid {
            width:parent.width;columns:2;columnSpacing:24;rowSpacing:6
            Repeater {
                model:[
                    {label:'Requests',value:details.compact(details.field('requests').amount)},
                    {label:'Recorded cost',value:details.field('cost').value || '—'},
                    {label:'Input tokens',value:details.compact(details.field('tokens').input)},
                    {label:'Output tokens',value:details.compact(details.field('tokens').output)},
                    {label:'Cached tokens',value:details.compact(details.field('cached').amount)},
                    {label:'Avg. latency',value:details.field('latency').value || '—'}
                ]
                Item {
                    required property var modelData
                    width:(details.width-24)/2;height:28
                    Text {anchors.verticalCenter:parent.verticalCenter;text:parent.modelData.label;font.pixelSize:11;color:details.theme.muted}
                    Text {anchors.right:parent.right;anchors.verticalCenter:parent.verticalCenter;text:parent.modelData.value;font.family:details.theme.font;font.pixelSize:12;color:details.theme.text}
                }
            }
        }
    }
}
