#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_ID="dev.scry.Scry"
BIN_DIR="${HOME}/.local/bin"
APP_DIR="${HOME}/.local/share/applications"
CONFIG_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}"
AUTOSTART_DIR="${CONFIG_DIR}/autostart"

stop_running() {
    pkill -x scry 2>/dev/null || return 0
    for _ in {1..25}; do
        pgrep -x scry >/dev/null || return 0
        sleep 0.2
    done
    pkill -9 -x scry 2>/dev/null || true
}

if [[ "${1:-}" == "--uninstall" ]]; then
    stop_running
    rm -f \
        "${BIN_DIR}/scry" \
        "${APP_DIR}/${APP_ID}.desktop" \
        "${AUTOSTART_DIR}/${APP_ID}.desktop"
    update-desktop-database "${APP_DIR}" 2>/dev/null || true
    echo "Uninstalled scry."
    exit 0
fi

cargo build --release --package scry-linux --manifest-path "${REPO_ROOT}/Cargo.toml"

mkdir -p "${BIN_DIR}" "${APP_DIR}"
install -m755 "${REPO_ROOT}/target/release/scry" "${BIN_DIR}/scry"
sed "s|@BIN@|${BIN_DIR}/scry|" "${REPO_ROOT}/gui/linux/data/${APP_ID}.desktop" > "${APP_DIR}/${APP_ID}.desktop"
update-desktop-database "${APP_DIR}" 2>/dev/null || true

case ":${PATH}:" in
    *":${BIN_DIR}:"*) ;;
    *) echo "Warning: ${BIN_DIR} is not on PATH." ;;
esac

stop_running
echo "Installed scry."
