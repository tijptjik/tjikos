import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Column {
    id:details
    required property var popup
    readonly property var theme:popup.theme
    readonly property var info:popup.panelData || ({})
    readonly property var settings:info.settings || ({})
    readonly property var schedule:info.schedule || ({})
    readonly property string storedStart:settings.start || '21:00'
    readonly property string storedEnd:settings.end || '07:30'
    readonly property bool busy:popup.feedback.busy===true
    readonly property bool needsKeyboard:citySearch.activeFocus || startTime.activeFocus || endTime.activeFocus
    property int temperatureDraft:settings.temperature || 5500
    property int gammaDraft:settings.gamma || 80
    spacing:16
    onSettingsChanged:{
        if(!warmth?.pressed && !temperatureDelay?.running)temperatureDraft=settings.temperature || 5500;
        if(!brightness?.pressed && !gammaDelay?.running)gammaDraft=settings.gamma || 80;
    }
    function act(name,value){popup.action(name,String(value));}
    function inspect(){return {enabled:info.enabled,active:info.active,mode:settings.mode,temperature:temperatureDraft,gamma:gammaDraft,sunrise:schedule.sunrise,sunset:schedule.sunset,location:settings.location?.name || '',needsKeyboard:needsKeyboard};}
    Timer {id:temperatureDelay;interval:350;onTriggered:details.act('temperature',details.temperatureDraft)}
    Timer {id:gammaDelay;interval:350;onTriggered:details.act('gamma',details.gammaDraft)}
    component Label: Text {
        font.family:details.theme.font;font.pixelSize:12;color:details.theme.text;textFormat:Text.PlainText
    }
    component Toggle: Switch {
        id:control
        width:parent.width;height:38;padding:0;hoverEnabled:true;focusPolicy:Qt.NoFocus
        enabled:!details.busy
        contentItem:Label {text:control.text;verticalAlignment:Text.AlignVCenter;rightPadding:54}
        indicator:Rectangle {
            x:control.width-width;y:(control.height-height)/2;width:38;height:22;radius:11
            color:control.checked?details.theme.accent:details.theme.surface
            border.width:control.checked?0:1;border.color:details.theme.border
            Rectangle {x:control.checked?18:3;y:3;width:16;height:16;radius:8;color:control.checked?details.theme.background:details.theme.text
                Behavior on x {NumberAnimation {duration:120}}}
        }
    }
    component SettingSlider: Slider {
        id:slider
        width:parent.width;height:26;enabled:!details.busy;focusPolicy:Qt.NoFocus
        background:Rectangle {
            x:slider.leftPadding;y:slider.topPadding+slider.availableHeight/2-height/2
            implicitHeight:4;width:slider.availableWidth;height:4;radius:2;color:details.theme.surface
            Rectangle {width:slider.visualPosition*parent.width;height:parent.height;radius:2;color:details.theme.accent}
        }
        handle:Rectangle {
            x:slider.leftPadding+slider.visualPosition*(slider.availableWidth-width)
            y:slider.topPadding+slider.availableHeight/2-height/2;width:16;height:16;radius:8
            color:details.theme.text;border.width:2;border.color:slider.pressed?details.theme.accent:details.theme.surface
        }
    }
    component Field: TextField {
        color:details.theme.text;placeholderTextColor:details.theme.muted;selectionColor:details.theme.accent
        selectedTextColor:details.theme.background;font.family:details.theme.font;font.pixelSize:12
        padding:10;height:38;enabled:!details.busy;selectByMouse:true
        background:Rectangle {radius:8;color:details.theme.surface;border.width:parent.activeFocus?1:0;border.color:details.theme.accent}
    }
    Column {
        width:parent.width;spacing:2
        Toggle {objectName:'nightlight-enabled';text:'Night light';checked:details.info.enabled===true;onClicked:{details.act('enabled',details.info.enabled?'off':'on');checked=Qt.binding(()=>details.info.enabled===true);}}
        Label {
            width:parent.width;wrapMode:Text.Wrap;color:details.theme.muted;font.pixelSize:11
            text:!details.info.enabled?'Off · enable to use your schedule':!details.info.running?'Night-light service unavailable':details.info.active?'Warm colours are active':details.schedule.polar?(details.schedule.polarNight?'Polar night · warm colours all day':'Midnight sun · daylight colours all day'):details.schedule.active?'Warm colours scheduled now':'Daylight colours · starts at '+details.schedule.nextTime
        }
    }
    Column {
        width:parent.width;spacing:6
        RowLayout {width:parent.width
            Label {text:'Colour temperature';Layout.fillWidth:true}
            Label {objectName:'nightlight-temperature-value';text:details.temperatureDraft+' K';color:details.theme.muted}
        }
        SettingSlider {id:warmth;objectName:'nightlight-temperature';from:1000;to:6500;stepSize:100;value:details.temperatureDraft
            onMoved:{details.temperatureDraft=Math.round(value/100)*100;if(!pressed)temperatureDelay.restart();}
            onPressedChanged:if(!pressed)temperatureDelay.restart()}
        RowLayout {width:parent.width;Label {text:'Warmer';font.pixelSize:10;color:details.theme.muted;Layout.fillWidth:true}Label {text:'Cooler';font.pixelSize:10;color:details.theme.muted}}
    }
    Column {
        width:parent.width;spacing:6
        RowLayout {width:parent.width
            Label {text:'Night-time brightness';Layout.fillWidth:true}
            Label {objectName:'nightlight-gamma-value';text:details.gammaDraft+'%';color:details.theme.muted}
        }
        SettingSlider {id:brightness;objectName:'nightlight-gamma';from:10;to:150;stepSize:5;value:details.gammaDraft
            onMoved:{details.gammaDraft=Math.round(value/5)*5;if(!pressed)gammaDelay.restart();}
            onPressedChanged:if(!pressed)gammaDelay.restart()}
    }
    Rectangle {width:parent.width;height:1;color:details.theme.surface}
    Column {
        width:parent.width;spacing:8
        Toggle {objectName:'nightlight-solar';text:'Follow sunset and sunrise';checked:details.settings.mode==='solar'
            onClicked:{details.act('mode',checked?'solar':'fixed');checked=Qt.binding(()=>details.settings.mode==='solar');}}
        RowLayout {
            width:parent.width;visible:details.settings.mode!=='solar';spacing:8
            Field {id:startTime;objectName:'nightlight-start';Layout.fillWidth:true;text:details.storedStart;inputMask:'99:99';Accessible.name:'Night light start time'}
            Label {text:'to';color:details.theme.muted}
            Field {id:endTime;objectName:'nightlight-end';Layout.fillWidth:true;text:details.storedEnd;inputMask:'99:99';Accessible.name:'Night light end time'}
            PanelButton {objectName:'nightlight-hours';theme:details.theme;showLabel:true;text:'Apply';enabled:!details.busy && startTime.acceptableInput && endTime.acceptableInput
                onClicked:details.act('hours',JSON.stringify({start:startTime.text,end:endTime.text}))}
        }
        RowLayout {
            width:parent.width;visible:details.settings.mode==='solar'
            Label {text:'Sunset  '+(details.schedule.sunset || '—');Layout.fillWidth:true}
            Label {text:'Sunrise  '+(details.schedule.sunrise || '—')}
        }
        Label {width:parent.width;text:details.schedule.timezone || '';font.pixelSize:10;color:details.theme.muted}
    }
    Rectangle {width:parent.width;height:1;color:details.theme.surface}
    Column {
        width:parent.width;spacing:8
        RowLayout {width:parent.width
            Column {Layout.fillWidth:true;spacing:3
                Label {text:'Location';font.pixelSize:11;color:details.theme.muted}
                Label {objectName:'nightlight-location';width:parent.width;elide:Text.ElideRight;text:details.settings.location?.name || 'Choose a location'}
            }
            PanelButton {objectName:'nightlight-locate';theme:details.theme;showLabel:true;text:'Use device';enabled:!details.busy;onClicked:details.act('locate','')}
        }
        Label {
            width:parent.width;wrapMode:Text.Wrap;font.pixelSize:10;color:details.theme.muted
            text:details.settings.location?.source==='device'?(details.info.locationAgeHours>24?'Last device location · update when you travel':''):'A selected city stays fixed until you change it.'
            visible:!!text
        }
        RowLayout {width:parent.width;spacing:8
            Field {id:citySearch;objectName:'nightlight-city-search';Layout.fillWidth:true;placeholderText:'Search for your city…';maximumLength:80;onAccepted:if(text.trim().length>=2)details.act('city-search',text.trim())}
            PanelButton {objectName:'nightlight-city-search-button';theme:details.theme;showLabel:true;text:'Search';enabled:!details.busy && citySearch.text.trim().length>=2;onClicked:details.act('city-search',citySearch.text.trim())}
        }
        Repeater {
            model:details.popup.nightlightPanel?(details.info.cities || []):[]
            delegate:Button {
                id:city;required property var modelData
                objectName:'nightlight-city-'+modelData.id
                width:parent.width;implicitHeight:42;padding:8;enabled:!details.busy
                background:Rectangle {radius:8;color:city.hovered?details.theme.surface:'transparent'}
                contentItem:Column {spacing:2
                    Label {width:parent.width;text:city.modelData.name;elide:Text.ElideRight}
                    Label {width:parent.width;text:city.modelData.detail || '';font.pixelSize:10;color:details.theme.muted;elide:Text.ElideRight}
                }
                onClicked:details.act('city-select',modelData.id)
            }
        }
    }
    Label {
        objectName:'nightlight-feedback';width:parent.width;wrapMode:Text.Wrap;font.pixelSize:11
        text:details.busy?'Applying…':details.popup.feedback.message || details.info.error || ''
        visible:!!text;color:details.popup.feedback.error || details.info.error?details.theme.accent:details.theme.muted
    }
}
