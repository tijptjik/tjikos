import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Column {
    id:details
    required property var popup
    readonly property var theme:popup.theme
    property var info:popup.panelData
    property string provider:popup.provider
    property var feedback:popup.feedback
    property var statusRow:popup.row
    property string actionModule:popup.selected?.context.module || ''
    readonly property bool busy:feedback.busy===true
    readonly property string cityCountry:info.countryCode || ''
    property bool citiesExpanded:false
    onCityCountryChanged:citiesExpanded=false
    onProviderChanged:citiesExpanded=false
    Connections {
        target:details.popup
        function onOpenedChanged(){if(!details.popup.opened)details.citiesExpanded=false;}
    }
    spacing:12
    function action(name,selection){popup.actionFor(actionModule,name,selection);}
    Text {width:parent.width;visible:!!details.statusRow?.state?.stale;text:'Cached · waiting for an update';font.pixelSize:11;color:details.theme.muted}
    Item {
        id:mullvadStatus
        readonly property real cityHeight:cityOptions.implicitHeight>0?cityOptions.implicitHeight+12:0
        readonly property real collapsedHeight:vpnContents.implicitHeight+24-cityHeight
        width:parent.width;height:collapsedHeight+(details.citiesExpanded?cityHeight:0)
        visible:details.provider==='mullvad'
        Rectangle {
            id:vpnSurface;objectName:'mullvad-status-surface'
            // Keep the section and its clipped city rows in step with the card.
            // The wrapper above supplies the destination height for layout.
            width:parent.width;height:Math.max(mullvadStatus.collapsedHeight,mullvadStatus.height+details.popup.renderedHeight-details.popup.targetHeight)
            radius:10;color:details.theme.surface;clip:true
            Column {
                id:vpnContents;x:12;y:12;width:parent.width-24;spacing:12
                RowLayout {
                    width:parent.width
                    Column {
                        Layout.fillWidth:true;spacing:4
                        Text {text:details.info.connection==='connected'?'Connected':details.info.connection==='connecting'?'Connecting…':details.info.connection==='disconnected'?'Disconnected':'Unavailable';font.family:details.theme.font;font.pixelSize:14;font.weight:Font.DemiBold;color:details.theme.text}
                        Text {text:details.info.connection==='connected'?'Traffic protected by Mullvad':'VPN connection';font.family:details.theme.font;font.pixelSize:11;color:details.theme.muted}
                    }
                    PanelButton {theme:details.theme;symbol:'power_settings_new';text:details.info.connection==='connected'?'Disconnect':'Connect';accented:details.info.connection==='connected';enabled:!details.busy && !!details.info.connection;onClicked:details.action(details.info.connection==='connected'?'disconnect':'connect')}
                }
                Rectangle {width:parent.width;height:1;color:details.theme.border;opacity:.45}
                RowLayout {
                    width:parent.width;spacing:10
                    Image {Layout.preferredWidth:32;Layout.preferredHeight:24;visible:!!details.info.countryCode;source:visible?'icons/flags/'+details.info.countryCode+'.svg':'';fillMode:Image.PreserveAspectFit;sourceSize:Qt.size(64,48)}
                    Column {
                        Layout.fillWidth:true;spacing:3
                        Text {text:details.info.country || 'Choose a country';font.family:details.theme.font;font.pixelSize:15;color:details.theme.text;textFormat:Text.PlainText}
                        Text {text:details.info.city || 'Select a location below';font.family:details.theme.font;font.pixelSize:12;color:details.theme.muted;textFormat:Text.PlainText}
                    }
                    PanelButton {
                        objectName:'mullvad-toggle-cities';theme:details.theme
                        symbol:details.citiesExpanded?'expand_more':'chevron_right'
                        text:details.citiesExpanded?'Hide cities':'Show cities';toolTipEnabled:false
                        enabled:!details.busy && (details.info.cities || []).length>0
                        onClicked:details.citiesExpanded=!details.citiesExpanded
                    }
                }
                Item {
                    width:parent.width;height:cityOptions.implicitHeight
                    Column {
                        id:cityOptions;objectName:'mullvad-cities'
                        width:parent.width;spacing:4
                        visible:details.citiesExpanded || vpnSurface.height>mullvadStatus.collapsedHeight+.01
                        enabled:details.citiesExpanded
                        Repeater {
                            model:details.info.cities || []
                            Button {
                                id:cityOption
                                required property var modelData
                                objectName:'mullvad-city-'+modelData.id
                                width:parent.width;height:32;padding:8
                                hoverEnabled:true;focusPolicy:Qt.NoFocus
                                enabled:!details.busy && !modelData.active
                                Accessible.name:modelData.name+(modelData.active?' (current)':'')
                                background:Rectangle {
                                    radius:6
                                    color:cityOption.hovered || cityOption.down?details.theme.background:'transparent'
                                    border.width:cityOption.hovered?1:0;border.color:details.theme.border
                                }
                                contentItem:RowLayout {
                                    Text {Layout.fillWidth:true;text:cityOption.modelData.name;font.family:details.theme.font;font.pixelSize:12;color:cityOption.modelData.active?details.theme.muted:details.theme.text;textFormat:Text.PlainText;elide:Text.ElideRight}
                                    Text {visible:cityOption.modelData.active;text:'Current';font.family:details.theme.font;font.pixelSize:11;color:details.theme.muted}
                                }
                                onClicked:{details.citiesExpanded=false;details.action('city',{country:details.cityCountry,city:modelData.id});}
                            }
                        }
                    }
                }
            }
        }
    }
    Column {
        width:parent.width;spacing:12;visible:details.provider==='tailscale'
        Rectangle {
            width:parent.width;height:tailContents.implicitHeight+24;radius:10;color:details.theme.surface
            Column {
                id:tailContents;x:12;y:12;width:parent.width-24;spacing:10
                RowLayout {
                    width:parent.width;spacing:8
                    Column {
                        Layout.fillWidth:true;spacing:4
                        Text {width:parent.width;text:details.info.device || 'This device';font.family:details.theme.font;font.pixelSize:14;font.weight:Font.DemiBold;color:details.theme.text;textFormat:Text.PlainText;elide:Text.ElideRight}
                        Text {text:details.info.connection==='Running'?'Connected':details.info.connection==='NeedsLogin'?'Sign-in required':details.info.connection==='Stopped'?'Disconnected':'Unavailable';font.family:details.theme.font;font.pixelSize:11;color:details.theme.muted}
                    }
                    PanelButton {objectName:'tailscale-toggle';theme:details.theme;symbol:'power_settings_new';text:details.info.connection==='Running'?'Disconnect':'Connect';accented:details.info.connection==='Running';enabled:!details.busy && !!details.info.connection;onClicked:details.action('toggle')}
                }
                RowLayout {
                    width:parent.width
                    Text {Layout.fillWidth:true;text:details.info.address || 'No address assigned';font.family:details.theme.font;font.pixelSize:12;color:details.theme.muted;textFormat:Text.PlainText;elide:Text.ElideMiddle}
                    PanelButton {theme:details.theme;symbol:'content_copy';text:'Copy this device address';enabled:!details.busy && !!details.info.address;onClicked:details.action('copy','self')}
                }
            }
        }
        Column {
            width:parent.width;spacing:6
            Repeater {
                model:details.info.peers || []
                Rectangle {
                    id:peer
                    required property var modelData
                    width:parent.width;height:58;radius:8;color:peerHover.hovered?details.theme.surface:'transparent'
                    HoverHandler {id:peerHover;blocking:false}
                    RowLayout {
                        anchors.fill:parent;anchors.leftMargin:8;anchors.rightMargin:4;spacing:8
                        Rectangle {width:6;height:6;radius:3;color:peer.modelData.online?details.theme.accent:details.theme.muted;opacity:peer.modelData.online?1:.4}
                        Column {
                            Layout.fillWidth:true;spacing:4
                            Text {width:parent.width;text:peer.modelData.name;font.family:details.theme.font;font.pixelSize:13;color:details.theme.text;textFormat:Text.PlainText;elide:Text.ElideRight}
                            Text {width:parent.width;text:(peer.modelData.online?'Online':'Offline')+(peer.modelData.address?' · '+peer.modelData.address:'');font.family:details.theme.font;font.pixelSize:11;color:details.theme.muted;textFormat:Text.PlainText;elide:Text.ElideMiddle}
                        }
                        PanelButton {objectName:'tailscale-ping-'+modelData.id;theme:details.theme;symbol:'network_ping';text:'Check reachability';enabled:!details.busy && peer.modelData.online && details.info.connection==='Running';onClicked:details.action('ping',peer.modelData.id)}
                        PanelButton {objectName:'tailscale-copy-'+modelData.id;theme:details.theme;symbol:'content_copy';text:'Copy device address';enabled:!details.busy && !!peer.modelData.address;onClicked:details.action('copy',peer.modelData.id)}
                    }
                }
            }
            Text {visible:!(details.info.peers || []).length;text:'No other devices to show';font.pixelSize:12;color:details.theme.muted}
        }
    }
    Column {
        width:parent.width;spacing:6;visible:details.provider==='datetime'
        Repeater {
            model:details.provider==='datetime'?details.popup.fields:[]
            Rectangle {
                required property var modelData
                width:parent.width;height:56;radius:8;color:details.theme.surface
                Column {
                    x:12;anchors.verticalCenter:parent.verticalCenter;spacing:4
                    Text {text:parent.parent.modelData.label;font.family:details.theme.font;font.pixelSize:13;color:details.theme.text;textFormat:Text.PlainText}
                    Text {text:parent.parent.modelData.detail;font.family:details.theme.font;font.pixelSize:10;color:details.theme.muted;textFormat:Text.PlainText}
                }
                Text {anchors.right:parent.right;anchors.rightMargin:12;anchors.verticalCenter:parent.verticalCenter;text:parent.modelData.value;font.family:details.theme.font;font.pixelSize:23;color:details.theme.text;textFormat:Text.PlainText}
            }
        }
    }
    Column {
        width:parent.width;spacing:6;visible:details.provider==='locale'
        Repeater {
            model:details.provider==='locale'?details.popup.fields:[]
            Button {
                id:method
                required property var modelData
                objectName:'input-method-'+(modelData.id || '')
                width:parent.width;height:54;padding:10;hoverEnabled:true;focusPolicy:Qt.NoFocus
                enabled:!!modelData.id && !details.busy
                Accessible.name:modelData.detail || modelData.label
                background:Rectangle {radius:8;color:method.hovered || method.modelData.active?details.theme.surface:'transparent'}
                contentItem:RowLayout {
                    spacing:12
                    Rectangle {
                        Layout.preferredWidth:34;Layout.preferredHeight:32;radius:7;color:details.theme.background
                        Text {anchors.centerIn:parent;text:method.modelData.label;font.family:details.theme.font;font.pixelSize:15;color:details.theme.text;textFormat:Text.PlainText}
                    }
                    Column {
                        Layout.fillWidth:true;spacing:3
                        Text {width:parent.width;text:method.modelData.detail || method.modelData.label;font.family:details.theme.font;font.pixelSize:12;color:details.theme.text;textFormat:Text.PlainText;elide:Text.ElideRight}
                        Text {visible:method.modelData.active===true;text:'Current input method';font.family:details.theme.font;font.pixelSize:10;color:details.theme.muted}
                    }
                    Text {visible:method.modelData.active===true;text:'check';font.family:'Material Symbols Rounded';font.pixelSize:20;color:details.theme.accent}
                }
                onClicked:details.action('select',modelData.id)
            }
        }
    }
    Column {
        objectName:'tailscale-footer'
        width:parent.width;spacing:10;visible:details.provider==='tailscale'
        Rectangle {width:parent.width;height:1;color:details.theme.surface}
        RowLayout {
            width:parent.width;spacing:8
            Text {text:'devices';font.family:'Material Symbols Rounded';font.pixelSize:20;color:details.theme.muted}
            Text {
                Layout.fillWidth:true
                text:'Devices · '+(details.info.online || 0)+' / '+(details.info.total || 0)+' online'
                font.family:details.theme.font;font.pixelSize:11
                color:details.theme.text;opacity:0.65
                elide:Text.ElideRight;textFormat:Text.PlainText
            }
            PanelButton {theme:details.theme;symbol:'refresh';text:'Refresh devices';enabled:!details.busy;onClicked:details.action('refresh')}
            PanelButton {theme:details.theme;symbol:'open_in_new';text:'Manage devices';enabled:!details.busy;onClicked:details.action('admin')}
        }
    }
    Text {
        width:parent.width;visible:!!text && (details.provider!=='tailscale' || details.busy || details.feedback.error===true) && (details.provider!=='mullvad' || details.busy || details.feedback.error===true)
        text:details.busy?'Applying…':details.feedback.message || ''
        font.family:details.theme.font;font.pixelSize:11;color:details.feedback.error?details.theme.accent:details.theme.muted
        wrapMode:Text.WordWrap;textFormat:Text.PlainText
    }
}
