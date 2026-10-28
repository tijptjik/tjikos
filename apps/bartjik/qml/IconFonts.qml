import QtQuick

QtObject {
    readonly property FontLoader materialSymbols: FontLoader {
        source: Qt.resolvedUrl("fonts/MaterialSymbolsRounded.ttf")
        onStatusChanged: {
            if (status === FontLoader.Error)
                console.error("Cannot load the bundled Material Symbols Rounded font:", source)
        }
    }
}
