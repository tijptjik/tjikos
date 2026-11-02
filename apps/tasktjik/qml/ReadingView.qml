import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io

KineticScrollView {
    id: root
    required property var theme
    property var sites: []
    property bool grid: false
    property bool categories: false
    property string selectedId: ''
    property var previews: ({})
    property var queue: []
    property string loadingUrl: ''
    property bool ready: false
    signal selectSite(string id)
    signal openSite(string id)
    clip: true
    contentWidth: availableWidth
    ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
    readonly property var sections: categories ? Array.from(new Set(sites.map(s=>s.group || 'All'))) : ['']
    function domain(url) {return String(url || '').replace(/^https?:\/\//,'').split('/')[0].replace(/^www\./,'');}
    function path(url) {return String(url || '').replace(/^https?:\/\/[^/]+/,'') || '/';}
    function enqueue() {
        queue=Array.from(new Set(sites.map(s=>s.reference).filter(url=>/^https?:\/\//.test(url || '') && previews[url]===undefined && url!==loadingUrl)));
        next();
    }
    function next() {
        if(!ready || loadingUrl || fetcher.running || !queue.length)return;
        loadingUrl=queue[0];queue=queue.slice(1);fetcher.running=true;
    }
    onSitesChanged: if(ready)enqueue()
    Component.onCompleted: {ready=true;enqueue();}
    Process {
        id: fetcher
        command: ['python3','-B',decodeURIComponent(Qt.resolvedUrl('page_preview.py').toString().replace('file://',''))]
        stdinEnabled: true
        property string output: ''
        onStarted: {output='';write(root.loadingUrl);stdinEnabled=false;}
        stdout: StdioCollector {onStreamFinished:fetcher.output=text}
        onExited: {
            let value={};try {value=JSON.parse(output);}catch(e){}
            root.previews=Object.assign({},root.previews,{[root.loadingUrl]:value});
            root.loadingUrl='';stdinEnabled=true;Qt.callLater(root.next);
        }
    }
    function visualCards() {
        const rows=[];
        for(let s=0;s<sectionRows.count;s++) {
            const section=sectionRows.itemAt(s);
            for(let i=0;i<section.cards.count;i++) {
                const card=section.cards.itemAt(i),p=card.mapToItem(contentItem.contentItem,0,0);
                rows.push({id:card.site.id,x:p.x,y:p.y,height:card.height});
            }
        }
        return rows.sort((a,b)=>Math.abs(a.y-b.y)>1?a.y-b.y:a.x-b.x);
    }
    function navigate(direction, horizontal) {
        const rows=visualCards(),index=rows.findIndex(s=>s.id===selectedId);
        if(!rows.length)return;
        let next=rows[Math.max(0,Math.min(rows.length-1,index+direction))];
        if(!horizontal && index>=0) {
            const current=rows[index],candidates=rows.filter(r=>(r.y-current.y)*direction>1);
            candidates.sort((a,b)=>Math.abs(a.y-current.y)-Math.abs(b.y-current.y) || Math.abs(a.x-current.x)-Math.abs(b.x-current.x));
            next=candidates[0];
        }
        if(next){selectSite(next.id);Qt.callLater(revealSelection);}
    }
    function revealSelection() {
        stopWheelScroll();
        const row=visualCards().find(r=>r.id===selectedId);
        if(!row)return;
        if(row.y<contentItem.contentY)contentItem.contentY=row.y;
        else if(row.y+row.height>contentItem.contentY+height)contentItem.contentY=row.y+row.height-height;
    }
    Column {
        width:root.availableWidth
        spacing:22
        Repeater {
            id:sectionRows
            model:root.sections
            Column {
                id:section
                required property string modelData
                property alias cards: cards
                width:parent.width;spacing:10
                Label {
                    visible:root.categories
                    text:(section.modelData==='All'?'Inbox':section.modelData)+' · '+cards.count
                    color:root.theme.muted;font.pixelSize:12;font.weight:Font.DemiBold
                }
                Grid {
                    width:parent.width;spacing:12
                    columns:root.grid?Math.max(1,Math.floor(width/260)):1
                    Repeater {
                        id:cards
                        model:root.sites.filter(s=>!root.categories || (s.group || 'All')===section.modelData)
                        Rectangle {
                            id:card
                            required property var modelData
                            readonly property var site: modelData
                            readonly property var preview: root.previews[site.reference] || ({})
                            width:(parent.width-(parent.columns-1)*parent.spacing)/parent.columns
                            height:root.grid?294:142
                            radius:12;color:root.theme.surface
                            border.color:root.selectedId===site.id?root.theme.accent:root.theme.border
                            Accessible.role:Accessible.Link
                            Accessible.name:site.title+' · '+root.domain(site.reference)
                            Accessible.onPressAction:root.openSite(site.id)
                            MouseArea {
                                anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor
                                onClicked:root.selectSite(card.site.id)
                                onDoubleClicked:root.openSite(card.site.id)
                            }
                            Rectangle {
                                id:art
                                x:8;y:8;width:root.grid?parent.width-16:Math.min(160,parent.width*0.28)
                                height:root.grid?130:parent.height-16
                                radius:8;clip:true;color:root.theme.background
                                Image {
                                    id:pageImage
                                    anchors.centerIn:parent
                                    width:card.preview.imageKind==='favicon'?48:parent.width
                                    height:card.preview.imageKind==='favicon'?48:parent.height
                                    source:card.preview.image || '';asynchronous:true
                                    fillMode:card.preview.imageKind==='favicon'?Image.PreserveAspectFit:Image.PreserveAspectCrop
                                    sourceSize.width:card.preview.imageKind==='favicon'?64:640;sourceSize.height:card.preview.imageKind==='favicon'?64:320
                                }
                                Column {
                                    anchors.centerIn:parent;width:parent.width-16;spacing:8
                                    visible:pageImage.status!==Image.Ready
                                    Label {anchors.horizontalCenter:parent.horizontalCenter;text:root.domain(card.site.reference).slice(0,1).toUpperCase() || '↗';font.pixelSize:32;color:root.theme.accent}
                                    Label {width:parent.width;text:root.previews[card.site.reference]===undefined?'Rendering page…':card.preview.renderError==='blocked'?'Site blocked preview':card.preview.renderError==='sign-in'?'Sign-in required':'Preview unavailable';horizontalAlignment:Text.AlignHCenter;color:root.theme.muted;font.pixelSize:11;elide:Text.ElideRight}
                                }
                            }
                            ColumnLayout {
                                x:root.grid?14:art.x+art.width+14
                                y:root.grid?148:14
                                width:card.width-x-14;height:root.grid?card.height-y-12:card.height-28
                                spacing:5
                                Label {Layout.fillWidth:true;text:root.domain(card.site.reference);textFormat:Text.PlainText;color:root.theme.accent;font.pixelSize:11;elide:Text.ElideRight}
                                Label {Layout.fillWidth:true;text:card.site.title;textFormat:Text.PlainText;color:root.theme.text;font.pixelSize:16;font.weight:Font.DemiBold;wrapMode:Text.Wrap;maximumLineCount:2;elide:Text.ElideRight}
                                Label {Layout.fillWidth:true;text:card.preview.description || root.path(card.site.reference);textFormat:Text.PlainText;color:root.theme.muted;font.pixelSize:12;wrapMode:Text.Wrap;maximumLineCount:2;elide:Text.ElideRight}
                                Item {Layout.fillHeight:true}
                                RowLayout {
                                    Layout.fillWidth:true
                                    Label {Layout.fillWidth:true;text:card.site.done?'Read':(card.site.tags || []).map(t=>'#'+t).join('  ');textFormat:Text.PlainText;color:root.theme.muted;font.pixelSize:11;elide:Text.ElideRight}
                                    Button {
                                        id:openButton
                                        text:'Open page ↗';Accessible.name:'Open '+card.site.title
                                        padding:4
                                        contentItem:Text {text:openButton.text;color:root.theme.accent;font.pixelSize:12}
                                        background:Rectangle {radius:4;color:openButton.hovered?root.theme.border:'transparent'}
                                        onClicked:root.openSite(card.site.id)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Column {
            visible:!root.sites.length;width:parent.width;spacing:8;topPadding:40
            Label {anchors.horizontalCenter:parent.horizontalCenter;text:'No pages here yet';color:root.theme.text;font.pixelSize:18}
            Label {anchors.horizontalCenter:parent.horizontalCenter;text:'Save a page or adjust your filters.';color:root.theme.muted;font.pixelSize:13}
        }
    }
}
