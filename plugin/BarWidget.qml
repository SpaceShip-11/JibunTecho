import QtQuick
import Quickshell
import Quickshell.Io

// Jibun Techo bar widget for Omarchy 4 (Quickshell).
// Runs `jibun status` once a minute and shows its "text" field.
// Click opens the planner TUI.

Item {
    id: root

    // set by the bar host
    property var bar

    property string statusText: "󰃭"
    property bool pending: false

    implicitHeight: bar ? bar.barSize : 26
    implicitWidth: label.implicitWidth + 16

    Text {
        id: label
        anchors.centerIn: parent
        text: root.statusText
        color: root.pending ? "#e5c07b" : (root.bar ? root.bar.foreground : "white")
        font.family: root.bar ? root.bar.fontFamily : "monospace"
        font.pixelSize: (root.bar && root.bar.fontSize) ? root.bar.fontSize : 13
    }

    Process {
        id: proc
        command: [Quickshell.env("HOME") + "/.local/bin/jibun", "status"]
        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    const j = JSON.parse(text)
                    root.statusText = j.text
                    root.pending = (j["class"] === "pending")
                } catch (e) {
                    console.warn("jibun: could not parse status output:", e)
                }
            }
        }
    }

    Timer {
        interval: 60000
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: proc.running = true
    }

    MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: {
            if (root.bar)
                root.bar.run("omarchy-launch-or-focus-tui jibun")
            proc.running = true
        }
    }
}
