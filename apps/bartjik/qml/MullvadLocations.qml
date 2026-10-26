import QtQuick

Column {
    id:locations
    required property var popup
    readonly property var theme:popup.theme
    readonly property var info:popup.panelData
    readonly property bool busy:popup.feedback.busy===true
    spacing:12
    Text {text:'Quick locations';font.family:locations.theme.font;font.pixelSize:12;color:locations.theme.muted}
    Row {
        width:parent.width;spacing:8
        Repeater {
            model:locations.info.countries || []
            CountryButton {
                required property var modelData
                objectName:'mullvad-country-'+modelData.id
                theme:locations.theme;country:modelData.id;text:modelData.name;active:modelData.active===true
                width:Math.max(32,(locations.width-8*(Math.max(5,(locations.info.countries || []).length)-1))/Math.max(5,(locations.info.countries || []).length))
                height:Math.round(width*3/4);enabled:!locations.busy
                onClicked:locations.popup.action('country',modelData.id)
            }
        }
    }
    Text {width:parent.width;visible:locations.info.locationsAvailable===false;text:'Locations are temporarily unavailable.';font.pixelSize:12;color:locations.theme.muted;wrapMode:Text.WordWrap}
}
