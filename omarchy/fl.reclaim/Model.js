.pragma library

var GLYPH = {
  disk: String.fromCodePoint(0xF02CA),
  worktree: String.fromCodePoint(0xF062C),
  cargo: String.fromCodePoint(0xF1617),
  dotnet: String.fromCodePoint(0xF031B),
  node: String.fromCodePoint(0xF0399),
  cache: String.fromCodePoint(0xF00E8),
  scan: String.fromCodePoint(0xF0450),
  reclaim: String.fromCodePoint(0xF00E2),
  decision: String.fromCodePoint(0xF05D6),
  remove: String.fromCodePoint(0xF0A7A)
}

var KIND_LABEL = {
  worktree: "Worktrees",
  cargo: "Rust targets",
  dotnet: ".NET bin and obj",
  node: "node_modules",
  cache: "Caches"
}

// Matches reclaim's default; the plan snapshot carries the configured value.
var DEFAULT_LOW_DISK_FREE_PERCENT = 15
var UNITS = ["B", "KiB", "MiB", "GiB", "TiB"]
var UNIT_STEP = 1024
var WHOLE_NUMBER_FROM = 100
var SECONDS_PER_MINUTE = 60
var MINUTES_PER_HOUR = 60
var HOURS_SHOWN_UP_TO = 48
var HOURS_PER_DAY = 24
var ACTIVE = "activating"
var ERROR_PREFIX = "Error: "
var GHOSTTY_ID_PREFIX = "com.mitchellh.ghostty"

function kindGlyph(kind) {
  return GLYPH[kind] || GLYPH.cache
}

function kindLabel(kind) {
  return KIND_LABEL[kind] || kind
}

function formatBytes(bytes) {
  var value = Number(bytes) || 0
  var unit = 0
  while (value >= UNIT_STEP && unit < UNITS.length - 1) {
    value /= UNIT_STEP
    unit++
  }
  if (unit === 0) return value + " B"
  if (value >= WHOLE_NUMBER_FROM) return Math.round(value) + " " + UNITS[unit]
  return value.toFixed(1) + " " + UNITS[unit]
}

function ago(unixSeconds, nowMs) {
  if (!unixSeconds) return "never"
  var minutes = Math.floor((nowMs / 1000 - unixSeconds) / SECONDS_PER_MINUTE)
  if (minutes < 1) return "just now"
  if (minutes < MINUTES_PER_HOUR) return minutes + " min ago"
  var hours = Math.floor(minutes / MINUTES_PER_HOUR)
  if (hours < HOURS_SHOWN_UP_TO) return hours + " h ago"
  return Math.floor(hours / HOURS_PER_DAY) + " d ago"
}

function idleValue(days) {
  return "idle " + days + " d"
}

function plural(count, word) {
  if (count === 1) return count + " " + word
  return count + " " + word + "s"
}

// `df -B1 --output=avail,size <dir>`: a header line, then "<free> <total>".
function parseDf(text) {
  var lines = String(text || "").trim().split("\n")
  if (lines.length < 2) return null
  var fields = lines[1].trim().split(/\s+/)
  return { free: Number(fields[0]), total: Number(fields[1]) }
}

// `systemctl --user is-active reclaim.service reclaim-low-disk.service reclaim-scan.service`:
// one state per line, in that order. Oneshot services are "activating" while they run.
function parseActivity(text) {
  var states = String(text || "").trim().split("\n")
  if (states[0] === ACTIVE || states[1] === ACTIVE) return "reclaiming"
  if (states[2] === ACTIVE) return "scanning"
  return ""
}

function freePercent(disk) {
  if (!disk || !disk.total) return 100
  return Math.floor(disk.free * 100 / disk.total)
}

function usedFraction(disk) {
  if (!disk || !disk.total) return 0
  return (disk.total - disk.free) / disk.total
}

function lowPercent(plan) {
  if (!plan) return DEFAULT_LOW_DISK_FREE_PERCENT
  return plan.low_disk_free_percent
}

function decisions(plan) {
  if (!plan) return []
  return plan.needs_decision
}

function reclaimable(plan) {
  if (!plan) return []
  return plan.reclaimable
}

function barGlyph(activity) {
  if (activity === "reclaiming") return GLYPH.reclaim
  if (activity === "scanning") return GLYPH.scan
  return GLYPH.disk
}

function statusLine(activity, low, plan, lastRun, nowMs) {
  if (activity === "reclaiming") return "Reclaiming space"
  if (activity === "scanning") return "Scanning"
  if (low) return "Low on space"
  if (lastRun) return "Last run " + ago(lastRun.finished_at, nowMs)
  if (plan) return "Scanned " + ago(plan.scanned_at, nowMs)
  return "Not scanned yet"
}

function tooltip(disk, plan, activity) {
  var parts = [formatBytes(disk.free) + " free"]
  if (plan) parts.push(formatBytes(plan.reclaimable_bytes) + " reclaimable")
  if (plan && plan.needs_decision.length > 0) parts.push(plural(plan.needs_decision.length, "decision"))
  if (activity !== "") parts.push(activity)
  return parts.join(" · ")
}

function lastRunValue(lastRun, nowMs) {
  if (!lastRun) return "never"
  if (lastRun.failed > 0) return ago(lastRun.finished_at, nowMs) + ", " + lastRun.failed + " failed"
  return ago(lastRun.finished_at, nowMs)
}

function lastFreedValue(lastRun) {
  if (!lastRun) return "—"
  return formatBytes(lastRun.freed_bytes)
}

function scannedValue(plan, nowMs) {
  if (!plan) return "never"
  return ago(plan.scanned_at, nowMs)
}

function reclaimableValue(plan) {
  if (!plan) return "—"
  return formatBytes(plan.reclaimable_bytes)
}

function scheduleValue(scheduled) {
  if (scheduled) return "Daily"
  return "Off"
}

// Ghostty in single-instance mode hands a new launch to the running instance, which ignores
// --working-directory and inherits the focused terminal's directory instead. `+new-window`
// is Ghostty's way to open a window in the running instance, and it honours the directory.
function terminalCommand(terminalId, path) {
  if (terminalId.indexOf(GHOSTTY_ID_PREFIX) === 0) return ["ghostty", "+new-window", "--working-directory=" + path]
  return ["xdg-terminal-exec", "--dir=" + path]
}

// anyhow prints "Error: <message>" on stderr; show the message alone.
function removeError(stderr) {
  var lines = String(stderr || "").trim().split("\n")
  var message = lines[lines.length - 1]
  if (message.indexOf(ERROR_PREFIX) === 0) message = message.slice(ERROR_PREFIX.length)
  if (message === "") return "Delete failed"
  return message
}
