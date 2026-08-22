#!/usr/bin/env bash
set -euo pipefail

# 构建可分发的 macOS .app。默认编译当前机器架构；可用
# OAIP_MAC_TARGET=aarch64-apple-darwin 或 x86_64-apple-darwin 覆盖。
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="${OAIP_MAC_APP_NAME:-Ontology}"
OUT_DIR="${OAIP_MAC_OUT_DIR:-${ROOT_DIR}/target/mac}"
APP_DIR="${OUT_DIR}/${APP_NAME}.app"
HOST_TARGET="$(rustc -vV | awk '/^host: / {print $2}')"
TARGET="${OAIP_MAC_TARGET:-${HOST_TARGET}}"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "macOS is required to build the .app (current OS: $(uname -s))" >&2
    exit 2
fi

cd "${ROOT_DIR}"

echo "==> build UI"
(
    cd ui
    npm ci
    npm run build
)

echo "==> build Rust runtime (${TARGET})"
if [[ "${TARGET}" == "${HOST_TARGET}" ]]; then
    cargo build --release --bin oaip
    RUNTIME_BIN="${ROOT_DIR}/target/release/oaip"
else
    cargo build --release --target "${TARGET}" --bin oaip
    RUNTIME_BIN="${ROOT_DIR}/target/${TARGET}/release/oaip"
fi

if [[ ! -x "${RUNTIME_BIN}" ]]; then
    echo "runtime binary not found: ${RUNTIME_BIN}" >&2
    exit 1
fi

rm -rf "${APP_DIR}"
mkdir -p     "${APP_DIR}/Contents/MacOS"     "${APP_DIR}/Contents/Resources/ui-dist"     "${APP_DIR}/Contents/Resources/examples"     "${APP_DIR}/Contents/Resources/domain-packs"

cp "mac/Info.plist" "${APP_DIR}/Contents/Info.plist"
cp "${RUNTIME_BIN}" "${APP_DIR}/Contents/Resources/oaip"
cp "scripts/ontology-mac-launcher.sh" "${APP_DIR}/Contents/MacOS/Ontology"
cp -R ui/dist/. "${APP_DIR}/Contents/Resources/ui-dist/"
cp ontology.odl "${APP_DIR}/Contents/Resources/ontology.odl"
cp -R examples/. "${APP_DIR}/Contents/Resources/examples/"
if [[ -d domain-packs ]]; then
    cp -R domain-packs/. "${APP_DIR}/Contents/Resources/domain-packs/"
fi
chmod 755 "${APP_DIR}/Contents/MacOS/Ontology" "${APP_DIR}/Contents/Resources/oaip"

# ad-hoc 签名让本地 Finder 启动行为稳定；正式发布时应替换为开发者证书签名。
if command -v codesign >/dev/null 2>&1; then
    codesign --force --deep --sign - "${APP_DIR}"
fi

echo ""
echo "✅ Mac app 已生成：${APP_DIR}"
echo "   启动：open "${APP_DIR}""
echo "   本地数据库：~/Library/Application Support/Ontology/ontology.db"
echo "   首次登录凭据：~/Library/Application Support/Ontology/first-run-credentials.txt"
echo "   云端同步：未启用"
