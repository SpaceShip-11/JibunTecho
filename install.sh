#!/usr/bin/env bash
# jibun install script for Omarchy 4 (Quickshell bar + Hyprland Lua config)
#
# Run from the project root (next to Cargo.toml and the plugin/ folder):
#   ./install.sh            build, install binary, install + enable bar widget, add popup window rule
#   ./install.sh update     rebuild, re-copy the plugin files, restart the shell
#   ./install.sh rules      only (re)write the Hyprland popup window rule
#   ./install.sh uninstall  remove widget, binary and window rule (your data is kept)
#
# Popup size:  JIBUN_W=1200 JIBUN_H=800 ./install.sh rules
# Popup top offset (px from top of screen, below the bar): JIBUN_TOP=50 ./install.sh rules
# Bar section: JIBUN_SECTION=center ./install.sh

set -euo pipefail

ACTION="${1:-install}"
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="$HOME/.local/bin"
BIN="$BIN_DIR/jibun"
PLUGIN_ID="szs.jibun"
PLUGIN_SRC="$PROJECT_DIR/plugin"
PLUGIN_DEST="$HOME/.config/omarchy/plugins/$PLUGIN_ID"
SECTION="${JIBUN_SECTION:-right}"   # left | center | right
HYPR_LUA="$HOME/.config/hypr/hyprland.lua"
APP_ID="org.omarchy.jibun"
WIN_W="${JIBUN_W:-1100}"
WIN_H="${JIBUN_H:-700}"

# Distance from the top of the screen to the popup: the bar's reserved height
# (read from Hyprland if possible) plus a small gap. Falls back to 40.
detect_top() {
  local r=""
  if command -v hyprctl >/dev/null 2>&1 && command -v jq >/dev/null 2>&1; then
    r="$(hyprctl monitors -j 2>/dev/null | jq -r '[.[] | select(.focused)][0].reserved[1] // empty' 2>/dev/null || true)"
  fi
  if [[ "$r" =~ ^[0-9]+$ ]] && (( r > 0 )); then echo $(( r + 8 )); else echo 40; fi
}
WIN_TOP="${JIBUN_TOP:-$(detect_top)}"

log()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!!\033[0m  %s\n' "$*" >&2; }
die()  { printf '\033[1;31mxx\033[0m  %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

build_and_install() {
  have cargo || die "cargo not found"
  [[ -f "$PROJECT_DIR/Cargo.toml" ]] || die "Cargo.toml not found in $PROJECT_DIR"
  log "Building (release)"
  (cd "$PROJECT_DIR" && cargo build --release)
  install -Dm755 "$PROJECT_DIR/target/release/jibun" "$BIN"
  log "Installed $BIN"
  "$BIN" status || warn "jibun status failed"
}

copy_plugin() {
  have omarchy || die "'omarchy' command not found. Is this an Omarchy 4 system?"
  [[ -f "$PLUGIN_SRC/manifest.json" && -f "$PLUGIN_SRC/BarWidget.qml" ]] \
    || die "plugin/manifest.json or plugin/BarWidget.qml missing in $PROJECT_DIR"
  log "Copying plugin to $PLUGIN_DEST"
  rm -rf "$PLUGIN_DEST"
  mkdir -p "$PLUGIN_DEST"
  cp "$PLUGIN_SRC/manifest.json" "$PLUGIN_SRC/BarWidget.qml" "$PLUGIN_DEST/"
  omarchy plugin validate "$PLUGIN_DEST" || warn "plugin validate reported problems (see above)"
}

enable_plugin() {
  omarchy-shell shell rescanPlugins || warn "rescanPlugins failed"
  log "Enabling $PLUGIN_ID in the $SECTION section"
  omarchy plugin enable "$PLUGIN_ID" "$SECTION" \
    || omarchy plugin enable "$PLUGIN_ID" --section "$SECTION" \
    || warn "enable failed; try: omarchy plugin list --json | grep jibun"
}

restart_shell() {
  log "Restarting the Omarchy shell"
  omarchy restart shell || warn "restart failed"
}

# Float + center + size the planner window so it behaves like a popup.
add_window_rule() {
  [[ -f "$HYPR_LUA" ]] || { warn "No $HYPR_LUA; skipping window rule"; return; }
  cp -n "$HYPR_LUA" "$HYPR_LUA.jibun.bak"
  sed -i '/-- jibun-begin/,/-- jibun-end/d' "$HYPR_LUA"
  printf '\n-- jibun-begin\nhl.window_rule({ match = { class = "^%s$" }, float = true, size = { %s, %s } })\nhl.window_rule({ match = { class = "^%s$" }, move = { "(monitor_w - %s) / 2", %s } })\n-- jibun-end\n' \
    "$APP_ID" "$WIN_W" "$WIN_H" "$APP_ID" "$WIN_W" "$WIN_TOP" >> "$HYPR_LUA"
  log "Added popup rule to $HYPR_LUA (${WIN_W}x${WIN_H}, centered horizontally, ${WIN_TOP}px from top)"
  if have hyprctl; then
    hyprctl reload >/dev/null 2>&1 || true
    local errs
    errs="$(hyprctl configerrors 2>/dev/null || true)"
    if [[ -n "${errs//[[:space:]]/}" ]]; then
      warn "Hyprland config errors:"
      printf '%s\n' "$errs" >&2
    fi
  fi
  return 0
}

remove_window_rule() {
  [[ -f "$HYPR_LUA" ]] && sed -i '/-- jibun-begin/,/-- jibun-end/d' "$HYPR_LUA"
  have hyprctl && hyprctl reload >/dev/null 2>&1 || true
}

case "$ACTION" in
  install)
    build_and_install
    copy_plugin
    enable_plugin
    add_window_rule
    restart_shell
    log "Done. Click the calendar date in the bar; the planner should open as a floating popup."
    log "If it tiles instead, open it and run: hyprctl clients | grep -i -B3 -A10 jibun   (check the 'class:' line)"
    ;;
  update)
    build_and_install
    copy_plugin
    restart_shell
    ;;
  rules)
    add_window_rule
    ;;
  uninstall)
    have omarchy && { omarchy plugin remove "$PLUGIN_ID" || true; }
    rm -rf "$PLUGIN_DEST" "$BIN"
    remove_window_rule
    have omarchy && restart_shell
    log "Removed. Your data in ~/.local/share/jibun/data.json was left alone."
    ;;
  *)
    die "Unknown action '$ACTION'. Use: install | update | rules | uninstall"
    ;;
esac
