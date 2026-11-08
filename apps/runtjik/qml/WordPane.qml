import QtQuick
import QtQuick.Controls

Item {
    id: pane
    required property var theme
    required property var settings
    required property var rowModel
    property var sourceCounts: ({})
    property var hiddenSources: []
    required property Item headerHost
    required property var rows
    required property string query
    required property int selected
    property bool pointerSelection: false
    property string termRowId: ''
    property int termIndex: -1
    onTermIndexChanged: Qt.callLater(revealTerm)
    onTermRowIdChanged: Qt.callLater(revealTerm)
    readonly property var selectedTerms: termsFor(rows[selected])
    readonly property bool navigatingTerms: termRowId !== '' && rows[selected]?.id === termRowId && termIndex >= 0 && termIndex < selectedTerms.length && rowVisible(rows[selected])
    function termsFor(row) { return (row?.terms || []).concat(row?.sense_synonyms || []); }
    function leaveTerms() {termRowId='';termIndex=-1;}
    function enterSelected() {
        const row=rows[selected];
        if(!row || row.disabled || !rowVisible(row))return;
        if(row.recent_word){activateRow(selected,false);return;}
        if(navigatingTerms){
            const target=selectedTerms[termIndex];
            leaveTerms();lookupWord(target.term || target.text);return;
        }
        let index=selected;
        if(!termsFor(row).length){
            index=rows.findIndex(candidate=>rowVisible(candidate) && !candidate.disabled &&
                ['synonyms','related_terms'].includes(candidate.section) && candidate.word_source===row.word_source &&
                candidate.language===row.language && candidate.etymology===row.etymology &&
                (!row.part_of_speech || candidate.part_of_speech===row.part_of_speech) && termsFor(candidate).length);
        }
        if(index<0)return;
        const options=termsFor(rows[index]);
        if(options.length===1){
            leaveTerms();lookupWord(options[0].term || options[0].text);return;
        }
        selectRow(index,false);
        termRowId=rows[index].id;termIndex=0;
        list.positionViewAtIndex(index,ListView.Contain);
    }
    function moveTerm(delta) {
        if(!navigatingTerms)return;
        termIndex=(termIndex+delta+selectedTerms.length)%selectedTerms.length;
    }
    function revealTerm() {
        if(!navigatingTerms)return;
        function find(item) {
            if(!item)return null;
            if(item.objectName==='wordTerm' && item.keyboardSelected)return item;
            for(const child of item.children || []){const match=find(child);if(match)return match;}
            return null;
        }
        const item=find(list.itemAtIndex(selected));
        if(!item)return;
        const y=item.mapToItem(list.contentItem,0,0).y;
        if(y<list.contentY+8)list.contentY=Math.max(list.originY,y-8);
        else if(y+item.height>list.contentY+list.height-8)list.contentY=Math.min(list.originY+list.contentHeight-list.height,y+item.height-list.height+8);
    }
    function localeTag(language) {
        return ({English:'EN',French:'FR',German:'DE',Dutch:'NL',Spanish:'ES',Portuguese:'PT',Italian:'IT',
            Chinese:'ZH',Cantonese:'YUE',Japanese:'JA',Korean:'KO',Arabic:'AR',Russian:'RU',Ukrainian:'UK',
            Polish:'PL',Swedish:'SV',Norwegian:'NO',Danish:'DA',Finnish:'FI',Hindi:'HI',Latin:'LA',Greek:'EL',
            Turkish:'TR',Vietnamese:'VI',Indonesian:'ID',Thai:'TH',Hebrew:'HE'})[language] || language;
    }
    readonly property bool synonyms: query.startsWith('/synonym')
    readonly property string term: query.replace(/^\/\S+\s*/, '').trim()
    readonly property bool recent: rows.some(row=>row.recent_word)
    readonly property bool empty: term.length === 0 && !recent
    readonly property bool loadingOnly: rows.length === 1 && rows[0].id === 'Words:loading'
    readonly property var sourceNames: ({wiktionary:'Wiktionary',urban_dictionary:'Urban Dictionary',free_dictionary:'Free Dictionary',datamuse:'Datamuse'})
    readonly property var providers: {
        const configured=settings.plugin_settings?.Words?.[synonyms?'synonym_sources':'define_sources'] || [];
        const names=configured.map(key=>sourceNames[key] || key);
        for(const row of rows){const name=rowSource(row);if(name && !names.includes(name))names.push(name);}
        return names.map(name=>({name:name,count:sourceCounts[name] ?? rows.filter(row=>rowSource(row)===name && !row.disabled).length}));
    }
    readonly property var visibleIndices: rows.map((row,index)=>!row.disabled && rowVisible(row)?index:-1).filter(index=>index>=0)
    readonly property bool allHidden: !recent && providers.length>0 && providers.every(provider=>hiddenSources.includes(provider.name))
    readonly property bool showIndicator: empty || rows.length === 0 || loadingOnly || allHidden
    function rowSource(row) {
        if(!row)return '';
        if(row.word_source)return row.word_source;
        return sourceNames[(row.id || '').split(':')[1]] || '';
    }
    function rowVisible(row) {return !!row && !hiddenSources.includes(rowSource(row));}
    function toggleSource(name) {
        const position=readingPosition();
        hiddenSources=hiddenSources.includes(name)?hiddenSources.filter(source=>source!==name):hiddenSources.concat([name]);
        if(!rowVisible(rows[selected]))selectRow(visibleIndices[0] ?? 0,false);
        const anchored=rows.find(row=>row.id===position.id);
        restoreReadingPosition(anchored && rowVisible(anchored)?position:{top:true});
        nearEnd();
    }
    function sourceIcon(name) {
        return 'icons/providers/'+(name==='Wiktionary'?'wiktionary':name==='Urban Dictionary'?'urban-dictionary':'dictionary')+'.svg';
    }
    property alias view: list
    function readingPosition() {
        if(!list.visible || list.atYBeginning)return {top:true};
        for(let offset=0;offset<20;++offset){
            const index=list.indexAt(1,list.contentY+offset);
            const item=list.itemAtIndex(index);
            if(item)return {id:item.modelData.id,y:item.y-list.contentY};
        }
        return {top:true};
    }
    function restoreReadingPosition(position) {
        list.forceLayout();
        if(position.top){list.positionViewAtBeginning();return;}
        const index=rows.findIndex(row=>row.id===position.id);
        if(index<0)return;
        list.positionViewAtIndex(index,ListView.Beginning);
        const item=list.itemAtIndex(index);
        if(item)list.contentY=item.y-position.y;
    }
    signal selectRow(int index, bool pointer)
    signal activateRow(int index, bool alternate)
    signal nearEnd()
    signal lookupWord(string term)
    function sectionTitle(section) {
        return ({definitions:'Definitions',synonyms:'Synonyms',related_terms:'Related Terms',derived_terms:'Derived Terms',pronunciation:'Pronunciation',etymology:'Etymology',alternative_forms:'Alternative Forms',antonyms:'Antonyms',usage_notes:'Usage Notes',inflections:'Inflections',translations:'Translations'})[section] || '';
    }
    component TermLinks: Flow {
        id: links
        required property var terms
        required property string rowId
        property int termOffset: 0
        spacing: 4
        Repeater {
            model: links.terms
            delegate: Column {
                required property var modelData
                required property int index
                width: Math.min(links.width, termLabel.implicitWidth)
                spacing: 2
                Text {
                    id: termLabel
                    objectName: "wordTerm"
                    readonly property bool keyboardSelected: pane.navigatingTerms && pane.termRowId===links.rowId && pane.termIndex===links.termOffset+index
                    width: Math.min(links.width, implicitWidth)
                    text: modelData.text || ''; textFormat: Text.PlainText
                    wrapMode: Text.Wrap
                    leftPadding: 7; rightPadding: 7; topPadding: 4; bottomPadding: 4
                    color: keyboardSelected ? pane.theme.text : pane.theme.accent
                    font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size - 1
                    Rectangle {
                        anchors.fill: parent; z: -1; radius: 6
                        visible: termLabel.keyboardSelected
                        color: Qt.alpha(pane.theme.accent,.2)
                    }
                    MouseArea {
                        anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                        onClicked: pane.lookupWord(modelData.term || modelData.text)
                    }
                }
                Text {
                    visible: !!modelData.qualifier
                    x: 7; width: parent.width - 14
                    text: modelData.qualifier || ''; textFormat: Text.PlainText; wrapMode: Text.Wrap
                    color: pane.theme.muted
                    font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size - 3
                }
            }
        }
    }

    Flickable {
        id: providerViewport
        parent: pane.headerHost
        anchors.fill: parent
        visible: pane.visible
        contentWidth: providerBar.width; contentHeight: height
        interactive: contentWidth > width
        flickableDirection: Flickable.HorizontalFlick
        boundsBehavior: Flickable.StopAtBounds
        clip: true
        Row {
            id: providerBar
            spacing: 8
            Repeater {
                model: pane.providers
                delegate: Rectangle {
                    id: providerButton
                    required property var modelData
                    readonly property bool enabledSource: !pane.hiddenSources.includes(modelData.name)
                    objectName: 'wordProvider:' + modelData.name
                    width: Math.min(providerViewport.width, providerLabel.implicitWidth + providerCount.implicitWidth + 64)
                    height: 34; radius: 9
                    color: enabledSource ? pane.theme.surface : 'transparent'
                    border.width: 1; border.color: enabledSource ? pane.theme.border : Qt.alpha(pane.theme.border, .5)
                    opacity: enabledSource ? 1 : .5
                    Image {
                        x: 10; anchors.verticalCenter: parent.verticalCenter
                        width: 20; height: 20; sourceSize.width: 40; sourceSize.height: 40
                        source: pane.sourceIcon(modelData.name)
                    }
                    Text {
                        id: providerLabel
                        x: 38; anchors.verticalCenter: parent.verticalCenter
                        width: parent.width - 56 - providerCount.width
                        text: modelData.name; textFormat: Text.PlainText; elide: Text.ElideRight
                        color: enabledSource ? pane.theme.text : pane.theme.muted
                        font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size - 2
                    }
                    Text {
                        id: providerCount
                        anchors.right: parent.right; anchors.rightMargin: 10; anchors.verticalCenter: parent.verticalCenter
                        text: modelData.count; color: enabledSource ? pane.theme.accent : pane.theme.muted
                        font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size - 3
                    }
                    Accessible.role: Accessible.CheckBox
                    Accessible.name: modelData.name + ', ' + modelData.count + ' results'
                    Accessible.checked: enabledSource
                    Accessible.onPressAction: pane.toggleSource(modelData.name)
                    MouseArea {anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: pane.toggleSource(modelData.name)}
                    ToolTip.visible: providerHover.hovered
                    ToolTip.text: (enabledSource ? 'Hide ' : 'Show ') + modelData.name
                    HoverHandler {id: providerHover}
                }
            }
        }
    }
    Column {
        anchors.centerIn: parent
        width: Math.min(parent.width - 64, 420)
        spacing: 14
        visible: pane.showIndicator
        // Tone-on-tone lettering has the quiet, embossed feel of a watermark.
        Item {
            anchors.horizontalCenter: parent.horizontalCenter
            width: 140; height: 88
            Text {
                anchors.centerIn: parent; anchors.verticalCenterOffset: 1
                text: pane.synonyms ? '≈' : 'Aa'
                font.family: pane.settings.ui.font; font.pixelSize: 72
                color: pane.theme.border; opacity: .65
            }
            Text {
                anchors.centerIn: parent
                text: pane.synonyms ? '≈' : 'Aa'
                font.family: pane.settings.ui.font; font.pixelSize: 72
                color: pane.theme.surface
            }
        }
        Text {
            width: parent.width
            text: pane.allHidden ? 'Sources hidden' : pane.empty ? (pane.synonyms ? 'Find the right word' : 'Look up a word') : 'Looking up “' + pane.term + '”…'
            textFormat: Text.PlainText
            horizontalAlignment: Text.AlignHCenter; wrapMode: Text.Wrap
            font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size + 2
            color: pane.theme.muted; opacity: .85
        }
        Text {
            width: parent.width
            text: pane.allHidden ? 'Choose a source above to show its results.' : pane.empty ? (pane.synonyms ? 'Type a word above to explore its synonyms.' : 'Type a word above for definitions and examples.') : 'Results appear as each source responds.'
            horizontalAlignment: Text.AlignHCenter; wrapMode: Text.Wrap
            font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size - 2
            color: pane.theme.muted; opacity: .6
        }
    }

    ListView {
        id: list
        anchors.fill: parent; anchors.margins: 12
        visible: !pane.showIndicator
        clip: true; spacing: 0
        model: pane.rowModel
        currentIndex: pane.selected
        highlightFollowsCurrentItem: false
        boundsBehavior: Flickable.StopAtBounds
        onContentYChanged: if(visible && contentY + height >= originY + contentHeight - 160) pane.nearEnd()
        onContentHeightChanged: Qt.callLater(()=>{if(list.visible && list.contentHeight<=list.height)pane.nearEnd();})
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
        HoverHandler {
            property point lastScenePosition: Qt.point(-1, -1)
            onPointChanged: {
                const position=point.scenePosition;
                if(position.x===lastScenePosition.x && position.y===lastScenePosition.y)return;
                lastScenePosition=position;
                if(!pane.visible || !list.visible)return;
                const local=list.mapFromItem(null,position.x,position.y);
                if(local.x<0 || local.y<0 || local.x>=list.width || local.y>=list.height)return;
                const index=list.indexAt(local.x+list.contentX,local.y+list.contentY);
                if(index>=0 && !pane.rows[index]?.disabled && pane.rowVisible(pane.rows[index]))pane.selectRow(index,true);
            }
        }
        delegate: Item {
            id: entry
            required property var row
            readonly property var modelData: row
            required property int index
            readonly property bool loading: modelData.id === 'Words:loading'
            readonly property bool disabled: !!modelData.disabled
            readonly property bool chosen: index === pane.selected && !disabled
            readonly property string source: modelData.word_source || ''
            readonly property string language: modelData.language || ''
            readonly property string section: modelData.section || (pane.synonyms ? 'synonyms' : 'definitions')
            readonly property var previous: pane.rows[index - 1] || ({})
            readonly property bool firstSource: source !== '' && (index === 0 || source !== (previous.word_source || ''))
            readonly property bool firstLanguage: !!language && (firstSource || language !== previous.language)
            readonly property bool firstGroup: !disabled && (firstSource || firstLanguage || section !== (previous.section || (pane.synonyms ? 'synonyms' : 'definitions')) || modelData.group !== previous.group)
            readonly property int depth: Math.min(modelData.sense_depth || 0, 4)
            width: list.width - 10
            readonly property bool shown: pane.rowVisible(modelData)
            visible: shown
            height: shown ? (loading ? 34 : card.y + card.height + 8) : 0
            Column {
                id: sourceHeading
                x: 16; y: entry.firstGroup && entry.index > 0 ? 8 : 0
                width: parent.width - 32
                spacing: 10
                Text {
                    visible: (pane.settings.plugin_settings?.Words?.sections || []).includes('etymology') && entry.firstGroup && entry.section !== 'etymology' && /^Etymology \d/.test(modelData.etymology || '') && modelData.etymology !== entry.previous.etymology
                    text: modelData.etymology || ''
                    color: pane.theme.muted
                    font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size - 1
                }
                Item {
                    visible: entry.firstGroup
                    width: parent.width; height: entry.firstGroup ? 26 : 0
                    Row {
                        spacing: 9; height: parent.height
                        Text {
                            id: groupTitle
                            anchors.verticalCenter: parent.verticalCenter
                            text: entry.section === 'definitions' ? (modelData.part_of_speech || (entry.source === 'Urban Dictionary' ? 'Slang' : 'Definitions')) : entry.section === 'etymology' ? (modelData.etymology || 'Etymology') : pane.sectionTitle(entry.section)
                            color: pane.theme.text
                            font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size + 1; font.bold: true
                        }
                        Rectangle {
                            visible: !!entry.language
                            anchors.verticalCenter: parent.verticalCenter
                            width: localeLabel.implicitWidth + 12; height: 20; radius: 5
                            color: 'transparent'; border.color: pane.theme.border
                            Text {
                                id: localeLabel
                                objectName: 'wordLocale'
                                anchors.centerIn: parent
                                text: pane.localeTag(entry.language); textFormat: Text.PlainText
                                color: pane.theme.muted
                                font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size - 4
                            }
                        }
                    }
                    Text {
                        anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                        visible: !!modelData.url
                        text: '↗'; width: 24; horizontalAlignment: Text.AlignRight
                        color: pane.theme.muted
                        font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size - 3
                        MouseArea {
                            anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                            onClicked: pane.activateRow(entry.index, false)
                        }
                    }
                }
                Text {
                    visible: entry.firstGroup && entry.section !== 'definitions' && !!modelData.part_of_speech
                    text: modelData.part_of_speech || ''
                    color: pane.theme.muted
                    font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size - 3
                }
                Text {
                    visible: entry.firstGroup && entry.section === 'definitions' && !!modelData.headword
                    width: parent.width
                    text: modelData.headword || ''; textFormat: Text.PlainText; wrapMode: Text.Wrap
                    color: pane.theme.muted
                    font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size - 1
                }
            }
            Text {
                anchors.centerIn: parent
                visible: entry.loading
                text: 'Checking remaining sources…'
                color: pane.theme.muted; opacity: .6
                font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size - 3
            }
            Rectangle {
                id: card
                visible: !entry.loading
                y: sourceHeading.y + sourceHeading.height + (entry.firstGroup ? 18 : 0)
                x: entry.depth * 18
                width: parent.width - x
                height: content.implicitHeight + 32
                radius: 10
                color: entry.chosen ? pane.theme.surface : 'transparent'
                Rectangle {x: 8; y: 16; width: 2; height: parent.height - 32; radius: 1; color: pane.theme.accent; visible: entry.chosen}
                MouseArea {
                    anchors.fill: parent
                    cursorShape: entry.disabled ? Qt.ArrowCursor : Qt.PointingHandCursor
                    onClicked: if(!entry.disabled){if(modelData.recent_word)pane.activateRow(entry.index,false);else pane.selectRow(entry.index,true);}
                }
                Text {
                    id: senseNumber
                    x: 24; width: Math.max(32, implicitWidth)
                    y: content.y + definitionText.baselineOffset - baselineOffset
                    horizontalAlignment: Text.AlignRight
                    visible: !!modelData.sense_number
                    text: (modelData.sense_number || '') + ((modelData.sense_number || '').includes('.') ? '' : '.')
                    color: pane.theme.muted
                    font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size - 2
                }
                Column {
                    id: content
                    x: modelData.sense_number ? senseNumber.x + senseNumber.width + 12 : 24
                    y: 16
                    width: parent.width - x - 16
                    spacing: 9
                    Text {
                        id: definitionText
                        objectName: 'wordDefinition'
                        visible: !!modelData.name
                        width: parent.width
                        text: modelData.name; textFormat: Text.PlainText
                        wrapMode: Text.Wrap
                        color: entry.disabled ? pane.theme.muted : pane.theme.text
                        font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size
                        lineHeight: 1.3
                    }
                    Text {
                        visible: !!modelData.context
                        width: parent.width
                        text: modelData.context || ''; textFormat: Text.PlainText; wrapMode: Text.Wrap
                        color: pane.theme.muted
                        font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size - 2
                    }
                    TermLinks {width: parent.width; rowId: modelData.id; terms: modelData.terms || []; visible: terms.length > 0}
                    Repeater {
                        model: entry.disabled ? [] : (modelData.examples || []).filter(text => text.trim() !== '')
                        delegate: Text {
                            required property string modelData
                            width: content.width
                            text: modelData; textFormat: Text.PlainText; wrapMode: Text.Wrap
                            color: pane.theme.muted
                            font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size - 1; font.italic: true
                            lineHeight: 1.3
                        }
                    }
                    Column {
                        width: parent.width; spacing: 6
                        visible: (modelData.sense_synonyms || []).length > 0
                        Text {
                            text: 'Synonyms'; color: pane.theme.muted
                            font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size - 3
                        }
                        TermLinks {width: parent.width; rowId: modelData.id; termOffset: (modelData.terms || []).length; terms: modelData.sense_synonyms || []}
                    }
                    Text {
                        visible: entry.disabled
                        width: parent.width
                        text: modelData.subtitle || ''; textFormat: Text.PlainText; wrapMode: Text.Wrap
                        color: pane.theme.muted
                        font.family: pane.settings.ui.font; font.pixelSize: pane.settings.ui.font_size - 2
                    }
                }


            }
        }
    }
}
