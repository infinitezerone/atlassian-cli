#!/usr/bin/env bash
set -euo pipefail

# atlassian-cli 一键安装脚本
#
# 用法:
#   curl -fsSL https://raw.githubusercontent.com/infinitezerone/atlassian-cli/main/install.sh | sh
#   ./install.sh -v v0.2.0       # 下载指定版本
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
  echo "  -v 版本   指定发布版本 (如 v0.2.0)"
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

case "$OS" in
  darwin)
    case "$ARCH" in
      arm64 | aarch64) FILE="atlassian-cli-aarch64-apple-darwin.tar.gz" ;;
      x86_64 | amd64)  FILE="atlassian-cli-x86_64-apple-darwin.tar.gz" ;;
      *) echo "暂不支持的架构: $ARCH"; exit 1 ;;
    esac
    ;;
  linux)
    case "$ARCH" in
      x86_64 | amd64) FILE="atlassian-cli-x86_64-unknown-linux-gnu.tar.gz" ;;
      *) echo "暂不支持的架构: $ARCH"; exit 1 ;;
    esac
    ;;
  *)
    echo "暂不支持的操作系统: $OS"
    exit 1
    ;;
esac

echo "检测到系统环境: ${OS}-${ARCH}, 安装路径: ${INSTALL_DIR}"

install_file() {
  local src="$1"
  cp "$src" "${INSTALL_DIR}/atlassian-cli"
  chmod +x "${INSTALL_DIR}/atlassian-cli"
  echo "✅ 安装成功: ${INSTALL_DIR}/atlassian-cli"

  # 自动检测 PATH 并写入环境变量配置文件 (.zshrc / .bashrc / .profile)
  if [[ ":$PATH:" != *":${INSTALL_DIR}:"* ]]; then
    echo "⚠️ 检测到 ${INSTALL_DIR} 尚未包含在环境变量 \$PATH 中，正在为您自动写入..."
    SHELL_RC=""
    if [ -f "$HOME/.zshrc" ] || [ "${SHELL:-}" = "/bin/zsh" ] || [ "${SHELL:-}" = "/opt/homebrew/bin/zsh" ]; then
      SHELL_RC="$HOME/.zshrc"
    elif [ -f "$HOME/.bashrc" ]; then
      SHELL_RC="$HOME/.bashrc"
    elif [ -f "$HOME/.profile" ]; then
      SHELL_RC="$HOME/.profile"
    fi

    if [ -n "$SHELL_RC" ]; then
      if ! grep -q "${INSTALL_DIR}" "$SHELL_RC" 2>/dev/null; then
        echo "" >> "$SHELL_RC"
        echo "# atlassian-cli PATH" >> "$SHELL_RC"
        echo "export PATH=\"${INSTALL_DIR}:\$PATH\"" >> "$SHELL_RC"
        echo "✅ 已成功将 ${INSTALL_DIR} 追加至 ${SHELL_RC}"
      fi
    fi
  fi

  echo ""
  echo "🎉 准备就绪！请运行 'exec \$SHELL' 刷新终端 (或打开新终端窗口)"
  echo "👉 运行 'atlassian-cli login' 完成首次接入配置"
  echo "🤖 运行 'atlassian-cli skill install' 一键部署官方 AI Agent Skill"
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
TMP_DIR="$(mktemp -d)"
if curl -fsSL "$RELEASE_URL" -o "${TMP_DIR}/${FILE}"; then
  if [[ "$FILE" == *.tar.gz ]]; then
    tar -xzf "${TMP_DIR}/${FILE}" -C "$TMP_DIR"
    install_file "${TMP_DIR}/atlassian-cli"
  else
    install_file "${TMP_DIR}/${FILE}"
  fi
  rm -rf "$TMP_DIR"
  exit 0
fi

rm -rf "$TMP_DIR"
echo "❌ 下载失败: 请检查网络或确认 Release 资产是否存在: ${RELEASE_URL}"
exit 1
