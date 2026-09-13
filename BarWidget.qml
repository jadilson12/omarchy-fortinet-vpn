import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

BarWidget {
    id: root
    moduleName: "jadilson12.fortinet-vpn"
    property var status: ({state: "unknown", display: {label: "VPN ?", heading: "Status unavailable", error: "Waiting for the status collector."}})
    readonly property bool connected: status.state === "connected"
    readonly property color tint: connected ? Color.accent : (status.state === "disconnected" || status.state === "degraded" ? Color.urgent : Color.foreground)
    implicitWidth: button.implicitWidth
    implicitHeight: button.implicitHeight

    function refresh() {
        if (probe.running) probe.write("refresh\n")
        else probe.running = true
    }
    function collectorFailed(message) {
        status = {state: "unknown", display: {label: "VPN ?", heading: "Status unavailable", error: message}}
    }
    function details() {
        var display = status.display
        var lines = ["FORTINET  ·  " + display.heading]
        if (connected) {
            lines.push(status.name + "  ·  " + status.ip)
            lines.push("Duration  " + status.duration)
            lines.push("Total  ↓ " + display.received + "    ↑ " + display.sent)
            lines.push(display.download !== null
                ? "Speed  ↓ " + display.download + "    ↑ " + display.upload
                : "Speed  waiting for next sample")
            lines.push("Tunnel  " + status.interface)
        }
        if (display.error) lines.push(display.error)
        if (status.checked_at_ms) lines.push("Last checked  " + new Date(status.checked_at_ms).toLocaleTimeString())
        lines.push("Click: open FortiClient  ·  Right-click: refresh")
        return lines.join("\n")
    }
    Process {
        id: probe
        stdinEnabled: true
        command: [decodeURIComponent(Qt.resolvedUrl("target/release/fortinet-vpn-status").toString().replace(/^file:\/\//, "")),
            "--watch", "--config", decodeURIComponent(Qt.resolvedUrl("config.toml").toString().replace(/^file:\/\//, ""))]
        stdout: SplitParser {
            splitMarker: "\n"
            onRead: data => {
                try {
                    var result = JSON.parse(data)
                    if (!result || typeof result.state !== "string" || !result.display
                            || typeof result.display.label !== "string" || typeof result.display.heading !== "string")
                        throw new Error("Invalid status")
                    root.status = result
                }
                catch (e) { root.collectorFailed("The status collector returned an invalid result. Rebuild the Rust binary.") }
            }
        }
        onRunningChanged: {
            if (!running) root.collectorFailed("The status collector stopped. Check that the Rust binary is built and executable.")
        }
        onExited: root.collectorFailed("The status collector stopped. Retrying shortly.")
    }
    Component.onCompleted: probe.running = true
    // Rust schedules checks. This timer only recovers from a stopped collector.
    Timer { interval: 5000; running: !probe.running; repeat: true; onTriggered: probe.running = true }
    WidgetButton {
        id: button
        anchors.fill: parent
        bar: root.bar
        hasVisualContent: true
        labelVisible: false
        fixedWidth: root.vertical ? root.barSize : content.implicitWidth + Style.spaceReal(22)
        tooltipText: root.details()
        onTooltipTextChanged: if (tooltipHovered && bar) bar.showTooltip(button, tooltipText)
        onPressed: function(b) {
            if (b === Qt.LeftButton) Quickshell.execDetached(["/opt/forticlient/gui/FortiClient"])
            else root.refresh()
        }
        Rectangle {
            anchors.centerIn: parent
            width: parent.width - Style.spaceReal(4)
            height: Math.min(parent.height - Style.spaceReal(6), Style.spaceReal(25))
            radius: height / 2
            color: Util.alpha(root.tint, button.tooltipHovered ? 0.18 : 0.09)
            border.width: 1
            border.color: Util.alpha(root.tint, button.tooltipHovered ? 0.65 : 0.32)
            Behavior on color { ColorAnimation { duration: 160 } }
            Behavior on border.color { ColorAnimation { duration: 160 } }
        }
        Row {
            id: content
            anchors.centerIn: parent
            spacing: Style.spaceReal(6)
            Canvas {
                id: shield
                width: Style.spaceReal(14); height: Style.spaceReal(16)
                property color stroke: root.tint
                property bool secure: root.connected
                onStrokeChanged: requestPaint()
                onSecureChanged: requestPaint()
                onWidthChanged: requestPaint()
                onHeightChanged: requestPaint()
                onPaint: {
                    var c = getContext("2d")
                    c.reset(); c.scale(width / 14, height / 16)
                    c.strokeStyle = stroke; c.lineWidth = 1.4; c.lineJoin = "round"; c.lineCap = "round"
                    c.beginPath(); c.moveTo(7, 1); c.lineTo(12.5, 3); c.lineTo(12, 9)
                    c.quadraticCurveTo(11, 12.5, 7, 15); c.quadraticCurveTo(3, 12.5, 2, 9)
                    c.lineTo(1.5, 3); c.closePath(); c.stroke()
                    c.beginPath()
                    if (secure) { c.moveTo(4, 7.5); c.lineTo(6, 9.5); c.lineTo(10, 5.5) }
                    else { c.moveTo(7, 5); c.lineTo(7, 8.5); c.moveTo(7, 10.5); c.lineTo(7, 11) }
                    c.stroke()
                }
            }
            Text {
                visible: !root.vertical
                anchors.verticalCenter: parent.verticalCenter
                text: root.status.display.label
                width: Math.min(implicitWidth, Style.spaceReal(400))
                elide: Text.ElideRight
                textFormat: Text.PlainText
                color: button.foreground
                font.family: button.fontFamily
                font.pixelSize: Style.font.caption
                font.weight: Font.DemiBold
                font.letterSpacing: 0.6
            }
        }
    }
}
