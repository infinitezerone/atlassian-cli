#!/usr/bin/env bash
set -euo pipefail

# atlassian-cli 一键安装脚本
#
# 用法:
#   ./install.sh                 # 优先安装本地编译产物(若有),否则从 GitHub Releases 下载
#   ./install.sh -v v0.1.0       # 下载指定版本
#   ./install.sh -l              # 强制安装本地编译产物 (target/release/atlassian-cli)

REPO_OWNER="${REPO_OWNER:-infinitezerone}"
REPO_NAME="${REPO_NAME:-atlassian-cli}"

INSTALL_DIR="/usr/local/bin"
if [ ! -w "$INSTALL_DIR" ]; then
  INSTALL_DIR="$HOME/.local/bin"
  mkdir -p "$INSTALL_DIR"
fi

usage() {
  echo "用法: $0 [-v 版本] [-l]"
  echo "  -v 版本   指定发布版本 (如 v0.1.0)"
  echo "  -l        强制使用本地编译产物 (target/release/atlassian-cli)"
}

VERSION=""
LOCAL=0
while getopts "v:lh" opt; do
  case "$opt" in
    v) VERSION="$OPTARG" ;;
    l) LOCAL=1 ;;
    h) usage; exit 0 ;;
    *) usage; exit 1 ;;
  esac
done

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64 | amd64) ARCH="x64" ;;
  arm64 | aarch64) ARCH="arm64" ;;
  *) echo "暂不支持的架构: $ARCH"; exit 1 ;;
esac
FILE="atlassian-cli-${OS}-${ARCH}"

echo "检测到系统环境: ${OS}-${ARCH}, 安装路径: ${INSTALL_DIR}"

install_file() {
  local src="$1"
  cp "$src" "${INSTALL_DIR}/atlassian-cli"
  chmod +x "${INSTALL_DIR}/atlassian-cli"
  echo "✅ 安装成功: ${INSTALL_DIR}/atlassian-cli"
  echo "👉 运行 'atlassian-cli login' 完成配置"
}

# 1) 本地产物优先
if [ "$LOCAL" = 1 ] || { [ -z "$VERSION" ] && [ -f "./target/release/atlassian-cli" ]; }; then
  echo "📦 使用本地编译产物..."
  install_file "./target/release/atlassian-cli"
  exit 0
fi

# 2) 远程 Release 下载
VER="${VERSION:-latest}"
RELEASE_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${VER}/${FILE}"
if [ "$VER" = "latest" ]; then
  RELEASE_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/latest/download/${FILE}"
fi

echo "🌐 正在从 GitHub Release 下载 ${FILE} ..."
TMP="$(mktemp)"
if curl -fsSL "$RELEASE_URL" -o "$TMP"; then
  install_file "$TMP"
  rm -f "$TMP"
  exit 0
fi
rm -f "$TMP"
echo "❌ 下载失败: 请检查网络或在本地运行: cargo build --release"
exit 1
