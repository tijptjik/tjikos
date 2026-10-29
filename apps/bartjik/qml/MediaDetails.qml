import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Widgets

ClippingRectangle {
    id:details
    required property var popup
    readonly property var media:popup.row?.media || {}
    readonly property var theme:popup.theme
    readonly property bool artworkReady:cover.ready
    property bool lyricsShown:false
    readonly property string lyricsStatus:lyrics.status
    radius:12;color:theme.background
    function act(action){if(popup.row)popup.controller.act(action,popup.selected.context,popup.selected.window,popup.row);}
    MouseArea {
        anchors.fill:parent;z:1
        acceptedButtons:Qt.MiddleButton
        onClicked:details.act('player.toggle')
        onWheel:event=>{event.accepted=false;}
    }
    AlbumArt {
        id:cover;anchors.top:parent.top;anchors.left:parent.left;anchors.right:parent.right
        height:Math.max(0,parent.height-footer.height)
        controller:popup.controller;requestedSource:details.media.artwork || '';visible:!details.lyricsShown
    }
    Text {anchors.centerIn:cover;visible:!details.lyricsShown && !cover.ready;text:'♫';color:details.theme.muted;font.pixelSize:64}
    Lyrics {
        id:lyrics;anchors.fill:cover;visible:details.lyricsShown
        controller:popup.controller;theme:details.theme;track:details.media
        playerId:popup.row?.id || ''
        active:details.lyricsShown && details.visible && popup.opened
    }
    PanelHandle {
        visible:details.popup.detached;popup:details.popup;theme:details.theme
        anchors.top:parent.top;anchors.right:parent.right;anchors.margins:10
        width:32;height:28;z:1
    }
    Rectangle {
        id:footer;anchors.left:parent.left;anchors.right:parent.right;anchors.bottom:parent.bottom
        height:64;color:details.theme.surface
        Row {
            anchors.centerIn:parent;spacing:12
            Repeater {
                model:[
                    {icon:'skip_previous',label:'Previous track',action:'player.previous',enabled:details.media.canPrevious},
                    {icon:details.media.playing?'pause':'play_arrow',label:details.media.playing?'Pause':'Play',action:'player.toggle',enabled:details.media.canToggle},
                    {icon:'skip_next',label:'Next track',action:'player.next',enabled:details.media.canNext}]
                ToolButton {
                    id:playbackButton
                    required property var modelData
                    width:42;height:40;enabled:!!modelData.enabled;opacity:enabled?1:.35
                    hoverEnabled:true;focusPolicy:Qt.NoFocus
                    Accessible.name:modelData.label
                    background:Rectangle {radius:8;color:playbackButton.hovered?Qt.rgba(1,1,1,.1):'transparent'}
                    contentItem:Text {text:playbackButton.modelData.icon;horizontalAlignment:Text.AlignHCenter;verticalAlignment:Text.AlignVCenter;font.family:'Material Symbols Rounded';font.pixelSize:24;color:details.theme.text}
                    HoverHandler {cursorShape:Qt.PointingHandCursor}
                    onClicked:details.act(modelData.action)
                }
            }
        }
        ToolButton {
            id:lyricsButton
            anchors.left:parent.left;anchors.leftMargin:12;anchors.verticalCenter:parent.verticalCenter
            width:32;height:32;checkable:true;checked:details.lyricsShown
            Accessible.name:checked?'Show album art':'Show lyrics'
            contentItem:Text {
                text:'lyrics';font.family:'Material Symbols Rounded';font.pixelSize:18
                horizontalAlignment:Text.AlignHCenter;verticalAlignment:Text.AlignVCenter
                color:details.theme.text;opacity:lyricsButton.checked?1:.4
            }
            background:Rectangle {
                radius:8;color:lyricsButton.hovered?Qt.rgba(1,1,1,.1):'transparent'
                border.width:lyricsButton.visualFocus?1:0;border.color:details.theme.text
            }
            onClicked:details.lyricsShown=!details.lyricsShown
        }
        ToolButton {
            id:shuffleButton
            anchors.right:parent.right;anchors.rightMargin:12;anchors.verticalCenter:parent.verticalCenter
            width:32;height:32;visible:!!details.media.canShuffle
            checkable:true;checked:!!details.media.shuffle
            Accessible.name:'Shuffle'
            contentItem:Text {
                text:'shuffle';font.family:'Material Symbols Rounded';font.pixelSize:18
                horizontalAlignment:Text.AlignHCenter;verticalAlignment:Text.AlignVCenter
                color:'#ffffff';opacity:shuffleButton.checked?1:.4
            }
            background:Rectangle {
                radius:8
                color:shuffleButton.hovered?Qt.rgba(1,1,1,.1):'transparent'
                border.width:shuffleButton.visualFocus?1:0;border.color:'#ffffff'
            }
            onClicked:details.act('player.shuffle')
        }
    }
}
