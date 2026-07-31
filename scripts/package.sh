#!/bin/bash
# Vortex 一键打包脚本
#
# 构建 Android Release APK + Rust Server（APK 自动嵌入二进制）
# 最终产物统一输出到 output/ 目录

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo "=== Vortex 打包 ==="
echo "项目目录: $PROJECT_DIR"

# 1. 构建 Android Release APK + 拷贝到 output/
echo ""
echo "[1/2] 构建 Android Release APK..."
cd "$PROJECT_DIR/vortex_app"
./gradlew copyApkToOutput

# 2. 构建 Rust Server（APK 通过 build.rs 自动嵌入）
echo ""
echo "[2/2] 构建 Rust Server（嵌入 APK）..."
cd "$PROJECT_DIR/server"
cargo build --release

# 3. 拷贝 Rust 二进制到 output/
mkdir -p "$PROJECT_DIR/output"
cp "$PROJECT_DIR/server/target/release/vortex" "$PROJECT_DIR/output/vortex"

# 4. 输出产物信息
echo ""
echo "=== 打包完成 ==="
ls -lh "$PROJECT_DIR/output/"
echo ""
echo "使用方法:"
echo "  $PROJECT_DIR/output/vortex run        # 一键启动"
echo "  $PROJECT_DIR/output/vortex install    # 安装 APK 到设备"
echo "  $PROJECT_DIR/output/vortex relay      # 启动 Relay 服务器"
