#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
TAURI_DIR="${REPO_ROOT}/src-tauri"
TAURI_CONFIG="${TAURI_DIR}/tauri.conf.json"
PUBLISH_DIR="${REPO_ROOT}/publish"
MAC_TARGET="aarch64-apple-darwin"
WINDOWS_TARGET="x86_64-pc-windows-msvc"
EXPECTED_WINDOWS_ICON="icons/ai-monitor/icon.ico"

fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

[[ "$(uname -s)" == "Darwin" ]] || fail "this release workflow must run on macOS"

require_command cargo
require_command cargo-xwin
require_command codesign
require_command file
require_command lipo
require_command makensis
require_command node
require_command pnpm
require_command rustup

for target in "${MAC_TARGET}" "${WINDOWS_TARGET}"; do
  rustup target list --installed | grep -Fxq "${target}" ||
    fail "Rust target is not installed: ${target} (run: rustup target add ${target})"
done

read_config_field() {
  node -e '
    const fs = require("node:fs");
    const config = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
    const value = process.argv[2].split(".").reduce((current, key) => current?.[key], config);
    if (typeof value !== "string" || value.length === 0) process.exit(1);
    process.stdout.write(value);
  ' "${TAURI_CONFIG}" "$1"
}

PRODUCT_NAME="$(read_config_field productName)" ||
  fail "could not read productName from ${TAURI_CONFIG}"
VERSION="$(read_config_field version)" ||
  fail "could not read version from ${TAURI_CONFIG}"
MAIN_BINARY_NAME="$(read_config_field mainBinaryName)" ||
  fail "could not read mainBinaryName from ${TAURI_CONFIG}"
NSIS_INSTALLER_ICON="$(read_config_field bundle.windows.nsis.installerIcon)" ||
  fail "could not read bundle.windows.nsis.installerIcon from ${TAURI_CONFIG}"
NSIS_UNINSTALLER_ICON="$(read_config_field bundle.windows.nsis.uninstallerIcon)" ||
  fail "could not read bundle.windows.nsis.uninstallerIcon from ${TAURI_CONFIG}"

[[ "${NSIS_INSTALLER_ICON}" == "${EXPECTED_WINDOWS_ICON}" ]] ||
  fail "NSIS installer must use ${EXPECTED_WINDOWS_ICON}, got: ${NSIS_INSTALLER_ICON}"
[[ "${NSIS_UNINSTALLER_ICON}" == "${EXPECTED_WINDOWS_ICON}" ]] ||
  fail "NSIS uninstaller must use ${EXPECTED_WINDOWS_ICON}, got: ${NSIS_UNINSTALLER_ICON}"

WINDOWS_ICON="${TAURI_DIR}/${EXPECTED_WINDOWS_ICON}"
[[ -f "${WINDOWS_ICON}" ]] ||
  fail "missing Windows installer icon: ${WINDOWS_ICON}"
file "${WINDOWS_ICON}" | grep -Fq "MS Windows icon resource" ||
  fail "invalid Windows installer icon: ${WINDOWS_ICON}"

node - "${TAURI_CONFIG}" "${TAURI_DIR}" <<'NODE' ||
const fs = require("node:fs");
const path = require("node:path");

const [configPath, tauriDir] = process.argv.slice(2);
const config = JSON.parse(fs.readFileSync(configPath, "utf8"));
const languageFiles = config.bundle?.windows?.nsis?.customLanguageFiles;

if (!languageFiles || typeof languageFiles !== "object") {
  throw new Error("bundle.windows.nsis.customLanguageFiles must be configured");
}

for (const [language, relativePath] of Object.entries(languageFiles)) {
  if (typeof relativePath !== "string" || relativePath.length === 0) {
    throw new Error(`invalid NSIS language file for ${language}`);
  }

  const absolutePath = path.resolve(tauriDir, relativePath);
  const relativeToTauri = path.relative(tauriDir, absolutePath);
  if (relativeToTauri.startsWith("..") || path.isAbsolute(relativeToTauri)) {
    throw new Error(`NSIS language file must stay inside src-tauri: ${relativePath}`);
  }
  if (!fs.statSync(absolutePath).isFile()) {
    throw new Error(`NSIS language file is not a file: ${relativePath}`);
  }
}
NODE
  fail "invalid NSIS language file configuration"

TARGET_DIR="$(
  cargo metadata \
    --manifest-path "${TAURI_DIR}/Cargo.toml" \
    --format-version 1 \
    --no-deps |
    node -e '
      let input = "";
      process.stdin.setEncoding("utf8");
      process.stdin.on("data", chunk => input += chunk);
      process.stdin.on("end", () => process.stdout.write(JSON.parse(input).target_directory));
    '
)"

[[ "${PUBLISH_DIR}" == "${REPO_ROOT}/publish" ]] ||
  fail "refusing to clean unexpected publish directory: ${PUBLISH_DIR}"
mkdir -p "${PUBLISH_DIR}"
find "${PUBLISH_DIR}" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +

cd "${REPO_ROOT}"

printf 'Building %s %s for macOS ARM64...\n' "${PRODUCT_NAME}" "${VERSION}"
pnpm tauri build \
  --target "${MAC_TARGET}" \
  --bundles app,dmg

printf 'Building %s %s for Windows x64 with cargo-xwin...\n' "${PRODUCT_NAME}" "${VERSION}"
pnpm tauri build \
  --runner cargo-xwin \
  --target "${WINDOWS_TARGET}" \
  --bundles nsis \
  --no-sign

MAC_APP_BINARY="${TARGET_DIR}/${MAC_TARGET}/release/bundle/macos/${PRODUCT_NAME}.app/Contents/MacOS/${MAIN_BINARY_NAME}"
MAC_DMG="${TARGET_DIR}/${MAC_TARGET}/release/bundle/dmg/${PRODUCT_NAME}_${VERSION}_aarch64.dmg"
WINDOWS_APP_BINARY="${TARGET_DIR}/${WINDOWS_TARGET}/release/${MAIN_BINARY_NAME}.exe"
WINDOWS_INSTALLER="${TARGET_DIR}/${WINDOWS_TARGET}/release/bundle/nsis/${PRODUCT_NAME}_${VERSION}_x64-setup.exe"

[[ -s "${MAC_APP_BINARY}" ]] || fail "missing macOS application binary: ${MAC_APP_BINARY}"
[[ -s "${MAC_DMG}" ]] || fail "missing macOS installer: ${MAC_DMG}"
[[ -s "${WINDOWS_APP_BINARY}" ]] || fail "missing Windows application binary: ${WINDOWS_APP_BINARY}"
[[ -s "${WINDOWS_INSTALLER}" ]] || fail "missing Windows installer: ${WINDOWS_INSTALLER}"

[[ "$(lipo -archs "${MAC_APP_BINARY}")" == "arm64" ]] ||
  fail "macOS application binary is not ARM64: ${MAC_APP_BINARY}"
codesign --verify --deep --strict --verbose=2 \
  "${TARGET_DIR}/${MAC_TARGET}/release/bundle/macos/${PRODUCT_NAME}.app"
file "${WINDOWS_APP_BINARY}" | grep -Eq 'PE32\+.*x86-64' ||
  fail "Windows application binary is not x86-64: ${WINDOWS_APP_BINARY}"

install -m 0644 "${MAC_DMG}" "${PUBLISH_DIR}/$(basename "${MAC_DMG}")"
install -m 0644 "${WINDOWS_INSTALLER}" "${PUBLISH_DIR}/$(basename "${WINDOWS_INSTALLER}")"

printf 'Published installers:\n'
find "${PUBLISH_DIR}" -mindepth 1 -maxdepth 1 -type f -print
