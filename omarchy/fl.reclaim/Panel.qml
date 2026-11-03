import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Model.js" as Model

// Reads the snapshots `reclaim` writes to $XDG_STATE_HOME/reclaim and drives its systemd user units.
// Scanning is slow and heavy on the disk, so the panel never scans by itself; it only shows the
// last scan or run, and starts one when asked.
Panel {
  id: root
  moduleName: "fl.reclaim"
  ipcTarget: "fl.reclaim"

  readonly property string home: Quickshell.env("HOME")
  readonly property string stateDir: stateBase() + "/reclaim"
  readonly property int listedDecisions: 5
  readonly property var runUnits: ["reclaim.service", "reclaim-low-disk.service", "reclaim-scan.service"]
  readonly property var timerUnits: ["reclaim.timer", "reclaim-low-disk.timer"]

  property var plan: null
  property var lastRun: null
  property var disk: ({ free: 0, total: 0 })
  property string activity: ""
  property bool scheduled: false
  property bool scheduleBusy: false
  property real now: Date.now()
  property string terminalId: ""
  property string removingPath: ""
  property var removeErrors: ({})

  readonly property bool low: Model.freePercent(disk) < Model.lowPercent(plan)
  readonly property bool failed: !!(lastRun && lastRun.failed > 0)
  readonly property bool idle: activity === ""
  readonly property var decisions: Model.decisions(plan)
  readonly property color dim: Qt.darker(root.bar.foreground, 1.4)

  function stateBase() {
    var xdg = Quickshell.env("XDG_STATE_HOME")
    if (xdg) return xdg
    return home + "/.local/state"
  }

  function refresh() {
    now = Date.now()
    // Snapshots are replaced by rename, which can drop a file watch, so read them on every refresh too.
    planFile.reload()
    lastRunFile.reload()
    if (!dfProc.running) dfProc.running = true
    if (!unitsProc.running) unitsProc.running = true
    if (!timerProc.running) timerProc.running = true
  }

  function startUnit(unit) {
    Quickshell.execDetached(["systemctl", "--user", "start", "--no-block", unit])
    refreshSoon.restart()
  }

  function setScheduled(enabled) {
    if (scheduleProc.running) return
    var verb = "disable"
    if (enabled) verb = "enable"
    scheduleBusy = true
    scheduleProc.command = ["systemctl", "--user", verb, "--now"].concat(timerUnits)
    scheduleProc.running = true
  }

  function usageColor() {
    if (low) return bar.urgent
    return bar.foreground
  }

  function emptyReclaimableText() {
    if (plan) return "Nothing to delete."
    return "No scan yet. Press the scan button."
  }

  function openTerminal(path) {
    close()
    Quickshell.execDetached(["setsid", "uwsm-app", "--"].concat(Model.terminalCommand(terminalId, path)))
  }

  function removeWorktree(path, reason) {
    if (removeProc.running) return
    var errors = Object.assign({}, removeErrors)
    delete errors[path]
    removeErrors = errors
    removingPath = path
    removeProc.command = [home + "/.local/bin/reclaim", "remove", path, "--accept", reason]
    removeProc.running = true
  }

  function finishRemove(exitCode) {
    if (exitCode !== 0) {
      var errors = Object.assign({}, removeErrors)
      errors[removingPath] = Model.removeError(removeStderr.text)
      removeErrors = errors
    }
    removingPath = ""
    refresh()
  }

  onOpenedChanged: if (opened) refresh()

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  FileView {
    id: planFile
    path: root.stateDir + "/plan.json"
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: root.plan = JSON.parse(text())
    onLoadFailed: root.plan = null
  }

  FileView {
    id: lastRunFile
    path: root.stateDir + "/last-run.json"
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: root.lastRun = JSON.parse(text())
    onLoadFailed: root.lastRun = null
  }

  Process {
    id: dfProc
    command: ["df", "-B1", "--output=avail,size", root.home]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var parsed = Model.parseDf(text)
        if (parsed) root.disk = parsed
      }
    }
  }

  Process {
    id: unitsProc
    command: ["systemctl", "--user", "is-active"].concat(root.runUnits)
    stdout: StdioCollector { waitForEnd: true; onStreamFinished: root.activity = Model.parseActivity(text) }
  }

  // `reclaim remove` judges the worktree again and refuses if it changed since the scan.
  // On success it drops the decision from plan.json, which the FileView above picks up.
  Process {
    id: removeProc
    stderr: StdioCollector { id: removeStderr; waitForEnd: true }
    onExited: function(exitCode) { root.finishRemove(exitCode) }
  }

  Process {
    id: terminalIdProc
    running: true
    command: ["xdg-terminal-exec", "--print-id"]
    stdout: StdioCollector { waitForEnd: true; onStreamFinished: root.terminalId = text.trim() }
  }

  Process {
    id: timerProc
    command: ["systemctl", "--user", "is-enabled", root.timerUnits[0]]
    stdout: StdioCollector { waitForEnd: true; onStreamFinished: root.scheduled = text.trim() === "enabled" }
  }

  Process {
    id: scheduleProc
    onExited: {
      root.scheduleBusy = false
      root.refresh()
    }
  }

  Timer { interval: 60000; running: true; repeat: true; triggeredOnStart: true; onTriggered: root.refresh() }
  Timer { interval: 2000; running: root.opened || !root.idle; repeat: true; onTriggered: root.refresh() }
  TextMetrics {
    id: sizeMetrics
    font.family: root.bar.fontFamily
    font.pixelSize: Style.font.bodySmall
    text: "999.9 MiB"
  }

  Timer { id: refreshSoon; interval: 700; onTriggered: root.refresh() }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: Model.barGlyph(root.activity)
    active: root.low || root.failed
    tooltipText: Model.tooltip(root.disk, root.plan, root.activity)
    onPressed: function(mouseButton) { root.toggle() }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(380))
    contentHeight: panel.fittedContentHeight(column.implicitHeight)

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }

      Column {
        id: column
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        spacing: Style.space(14)

        PanelHero {
          title: "Disk"
          meta: Model.statusLine(root.activity, root.low, root.plan, root.lastRun, root.now)
          foreground: root.bar.foreground
          fontFamily: root.bar.fontFamily
          iconComponent: Component {
            Text {
              textFormat: Text.PlainText
              text: Model.GLYPH.disk
              color: root.bar.foreground
              font.family: root.bar.fontFamily
              font.pixelSize: Style.font.display
            }
          }
          trailingControl: Component {
            Row {
              spacing: Style.space(6)

              PanelActionButton {
                anchors.verticalCenter: parent.verticalCenter
                iconText: Model.GLYPH.scan
                tooltipText: "Scan now"
                foreground: root.bar.foreground
                fontFamily: root.bar.fontFamily
                enabled: root.idle
                onClicked: root.startUnit("reclaim-scan.service")
              }

              PanelActionButton {
                anchors.verticalCenter: parent.verticalCenter
                iconText: Model.GLYPH.reclaim
                tooltipText: "Reclaim now"
                foreground: root.bar.foreground
                fontFamily: root.bar.fontFamily
                enabled: root.idle
                onClicked: root.startUnit("reclaim.service")
              }

              ToggleSwitch {
                anchors.verticalCenter: parent.verticalCenter
                checked: root.scheduled
                busy: root.scheduleBusy
                foreground: root.bar.foreground
                onToggled: root.setScheduled(!root.scheduled)
              }
            }
          }
        }

        Item {
          width: parent.width
          implicitHeight: Style.space(8)

          Rectangle {
            id: track
            anchors.fill: parent
            radius: height / 2
            color: Qt.rgba(root.bar.foreground.r, root.bar.foreground.g, root.bar.foreground.b, 0.12)
          }

          Rectangle {
            anchors.left: track.left
            anchors.verticalCenter: track.verticalCenter
            height: track.height
            radius: track.radius
            width: Math.max(track.height, track.width * Model.usedFraction(root.disk))
            color: root.usageColor()

            Behavior on width { NumberAnimation { duration: 320; easing.type: Easing.OutCubic } }
            Behavior on color { ColorAnimation { duration: 220 } }
          }
        }

        Row {
          width: parent.width
          spacing: Style.space(20)

          Column {
            width: (parent.width - parent.spacing) / 2
            spacing: Style.spacing.labelGap
            InfoPair { label: "Free"; value: Model.formatBytes(root.disk.free) }
            InfoPair { label: "Reclaimable"; value: Model.reclaimableValue(root.plan) }
            InfoPair { label: "Last run"; value: Model.lastRunValue(root.lastRun, root.now) }
            InfoPair { label: "Scanned"; value: Model.scannedValue(root.plan, root.now) }
          }

          Column {
            width: (parent.width - parent.spacing) / 2
            spacing: Style.spacing.labelGap
            InfoPair { label: "Used"; value: Model.formatBytes(root.disk.total - root.disk.free) }
            InfoPair { label: "Decisions"; value: String(root.decisions.length) }
            InfoPair { label: "Last freed"; value: Model.lastFreedValue(root.lastRun) }
            InfoPair { label: "Schedule"; value: Model.scheduleValue(root.scheduled) }
          }
        }

        PanelSeparator { foreground: root.bar.foreground }

        Column {
          width: parent.width
          spacing: Style.space(6)

          PanelSectionHeader {
            text: "RECLAIMABLE"
            foreground: root.bar.foreground
            fontFamily: root.bar.fontFamily
          }

          Text {
            visible: Model.reclaimable(root.plan).length === 0
            width: parent.width
            textFormat: Text.PlainText
            text: root.emptyReclaimableText()
            color: root.dim
            font.family: root.bar.fontFamily
            font.pixelSize: Style.font.bodySmall
            wrapMode: Text.Wrap
          }

          Repeater {
            model: Model.reclaimable(root.plan)

            ListRow {
              required property var modelData
              glyph: Model.kindGlyph(modelData.kind)
              primary: Model.kindLabel(modelData.kind)
              meta: Model.plural(modelData.count, "item")
              size: Model.formatBytes(modelData.bytes)
            }
          }
        }

        PanelSeparator {
          visible: root.decisions.length > 0
          foreground: root.bar.foreground
        }

        Column {
          visible: root.decisions.length > 0
          width: parent.width
          spacing: Style.space(6)

          PanelSectionHeader {
            text: "NEEDS A DECISION"
            foreground: root.bar.foreground
            fontFamily: root.bar.fontFamily
          }

          Repeater {
            model: root.decisions.slice(0, root.listedDecisions)

            ListRow {
              required property var modelData
              glyph: Model.GLYPH.decision
              primary: modelData.display
              secondary: modelData.reason
              note: Model.idleValue(modelData.idle_days)
              size: Model.formatBytes(modelData.bytes)
              clickable: true
              removable: root.idle
              removing: root.removingPath === modelData.path
              error: root.removeErrors[modelData.path] || ""
              onClicked: root.openTerminal(modelData.path)
              onRemoveConfirmed: root.removeWorktree(modelData.path, modelData.reason)
            }
          }

          Text {
            visible: root.decisions.length > root.listedDecisions
            textFormat: Text.PlainText
            text: "… and " + (root.decisions.length - root.listedDecisions) + " more: run reclaim status"
            color: root.dim
            font.family: root.bar.fontFamily
            font.pixelSize: Style.font.caption
          }
        }
      }
    }
  }

  component ListRow: Item {
    id: row

    property string glyph: ""
    property string primary: ""
    property string secondary: ""
    // Dim column before the size, centered on the row.
    property string meta: ""
    // Dim text at the right end of the secondary line.
    property string note: ""
    property string size: ""
    property bool clickable: false
    // Shows a delete button. The first click arms it, the second confirms.
    property bool removable: false
    property bool removing: false
    property string error: ""
    property bool armed: false
    signal clicked()
    signal removeConfirmed()

    function pressRemove() {
      if (armed) {
        armed = false
        removeConfirmed()
        return
      }
      armed = true
      disarm.restart()
    }

    function secondaryLine() {
      if (error !== "") return error
      if (removing) return "Removing…"
      if (armed) return "Deletes " + secondary + ". Click again to confirm."
      return secondary
    }

    // Armed and error lines must be read in full, so they wrap and take the note's place.
    function expanded() {
      return armed || error !== ""
    }

    function secondaryWrap() {
      if (expanded()) return Text.Wrap
      return Text.NoWrap
    }

    function noteLine() {
      if (expanded()) return ""
      return note
    }

    function secondaryColor() {
      if (error !== "" || armed) return root.bar.urgent
      return root.dim
    }

    function buttonColor() {
      if (armed) return root.bar.urgent
      return root.bar.foreground
    }

    function actionWidth() {
      if (!removable) return 0
      return removeButton.size
    }

    function gapBefore(text) {
      if (text === "") return 0
      return Style.space(12)
    }

    width: parent.width
    implicitHeight: labels.implicitHeight + Style.space(12)

    Rectangle {
      anchors.fill: parent
      radius: Style.cornerRadius
      visible: row.clickable && mouse.containsMouse
      color: Qt.rgba(root.bar.foreground.r, root.bar.foreground.g, root.bar.foreground.b, 0.08)
    }

    Text {
      id: icon
      textFormat: Text.PlainText
      anchors.left: parent.left
      anchors.leftMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      width: Style.space(18)
      text: row.glyph
      color: root.bar.foreground
      font.family: root.bar.fontFamily
      font.pixelSize: Style.font.title
    }

    Column {
      id: labels
      anchors.left: icon.right
      anchors.leftMargin: Style.space(10)
      anchors.right: metaText.left
      anchors.rightMargin: Style.space(10)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(2)

      Text {
        width: parent.width
        textFormat: Text.PlainText
        text: row.primary
        color: root.bar.foreground
        font.family: root.bar.fontFamily
        font.pixelSize: Style.font.bodySmall
        elide: Text.ElideMiddle
      }

      Item {
        width: parent.width
        visible: row.secondary !== ""
        implicitHeight: secondaryText.implicitHeight

        Text {
          id: secondaryText
          anchors.left: parent.left
          anchors.right: noteText.left
          anchors.rightMargin: row.gapBefore(row.noteLine())
          textFormat: Text.PlainText
          text: row.secondaryLine()
          color: row.secondaryColor()
          wrapMode: row.secondaryWrap()
          font.family: root.bar.fontFamily
          font.pixelSize: Style.font.caption
          elide: Text.ElideRight
        }

        Text {
          id: noteText
          anchors.right: parent.right
          textFormat: Text.PlainText
          text: row.noteLine()
          color: root.dim
          font.family: root.bar.fontFamily
          font.pixelSize: Style.font.caption
        }
      }
    }

    Text {
      id: metaText
      textFormat: Text.PlainText
      anchors.right: sizeText.left
      anchors.rightMargin: row.gapBefore(row.meta)
      anchors.verticalCenter: parent.verticalCenter
      text: row.meta
      color: root.dim
      font.family: root.bar.fontFamily
      font.pixelSize: Style.font.bodySmall
    }

    // Fixed width so sizes line up across rows whatever their length.
    Text {
      id: sizeText
      textFormat: Text.PlainText
      anchors.right: actionSlot.left
      anchors.rightMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      width: Math.ceil(sizeMetrics.advanceWidth)
      horizontalAlignment: Text.AlignRight
      text: row.size
      color: root.bar.foreground
      font.family: root.bar.fontFamily
      font.pixelSize: Style.font.bodySmall
    }

    MouseArea {
      id: mouse
      anchors.fill: parent
      hoverEnabled: true
      enabled: row.clickable
      cursorShape: Qt.PointingHandCursor
      onClicked: row.clicked()
    }

    // After the row's MouseArea so it takes its own clicks.
    Item {
      id: actionSlot
      anchors.right: parent.right
      anchors.rightMargin: Style.space(4)
      anchors.verticalCenter: parent.verticalCenter
      width: row.actionWidth()
      height: removeButton.size

      PanelActionButton {
        id: removeButton
        anchors.centerIn: parent
        visible: row.removable
        enabled: !row.removing
        iconText: Model.GLYPH.remove
        tooltipText: "Delete this worktree"
        foreground: row.buttonColor()
        hoverColor: root.bar.urgent
        fontFamily: root.bar.fontFamily
        onClicked: row.pressRemove()
      }
    }

    Timer { id: disarm; interval: 3000; onTriggered: row.armed = false }
  }

  component InfoPair: Row {
    property string label: ""
    property string value: ""

    width: parent.width
    spacing: Style.space(8)

    InfoLabel { text: label }
    Item { width: Math.max(0, parent.width - parent.children[0].implicitWidth - parent.children[2].implicitWidth - parent.spacing * 2); height: 1 }
    InfoValue { text: value }
  }

  component InfoLabel: Text {
    textFormat: Text.PlainText
    color: root.bar.foreground
    opacity: 0.6
    font.family: root.bar.fontFamily
    font.pixelSize: Style.font.bodySmall
  }

  component InfoValue: Text {
    textFormat: Text.PlainText
    color: root.bar.foreground
    font.family: root.bar.fontFamily
    font.pixelSize: Style.font.bodySmall
  }
}
