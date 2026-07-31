#!/bin/bash
# Vortex 一键打包脚本
#
# 构建 Android Release APK + Rust Server（APK 自动嵌入二进制）
# 最终产物: server/target/release/vortex（单个可执行文件）

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo "=== Vortex 打包 ==="
echo "项目目录: $PROJECT_DIR"

# 1. 构建 Android Release APK
echo ""
echo "[1/2] 构建 Android Release APK..."
cd "$PROJECT_DIR/vortex_app"
./gradlew assembleRelease

# 2. 构建 Rust Server（APK 通过 build.rs 自动嵌入）
echo ""
echo "[2/2] 构建 Rust Server（嵌入 APK）..."
cd "$PROJECT_DIR/server"
cargo build --release

# 3. 输出产物信息
BINARY="$PROJECT_DIR/server/target/release/vortex"
if [ -f "$BINARY" ]; then
    SIZE=$(ls -lh "$BINARY" | awk '{print $5}')
    echo ""
    echo "=== 打包完成 ==="
    echo "产物: $BINARY"
    echo "大小: $SIZE"
    echo ""
    echo "使用方法:"
    echo "  $BINARY run        # 一键启动"
    echo "  $BINARY install    # 安装 APK 到设备"
    echo "  $BINARY relay      # 启动 Relay 服务器"
else
    echo "错误: 构建产物未找到"
    exit 1
fi
