.PHONY: help build clean test install run docs lint format

# 项目根目录
ROOT_DIR := $(shell pwd)
ANDROID_DIR := $(ROOT_DIR)/android-app
RELAY_DIR := $(ROOT_DIR)/relay-server

# 默认目标
help:
	@echo "Vortex - Android 反向网络代理工具"
	@echo ""
	@echo "使用方法: make [目标]"
	@echo ""
	@echo "构建目标:"
	@echo "  build           构建 Android APK 和 Rust 服务器"
	@echo "  build-android   构建 Android APK"
	@echo "  build-relay     构建 Rust 中继服务器"
	@echo "  clean           清理构建产物"
	@echo ""
	@echo "安装和运行:"
	@echo "  install         安装 APK 到设备"
	@echo "  run             启动中继服务器"
	@echo "  start           启动 VPN 服务"
	@echo "  stop            停止 VPN 服务"
	@echo ""
	@echo "测试:"
	@echo "  test            运行所有测试"
	@echo "  test-android    运行 Android 测试"
	@echo "  test-relay      运行 Rust 测试"
	@echo ""
	@echo "代码质量:"
	@echo "  lint            运行代码检查"
	@echo "  format          格式化代码"
	@echo ""
	@echo "文档:"
	@echo "  docs            生成文档"
	@echo ""
	@echo "发布:"
	@echo "  release         构建发布版本"
	@echo "  package         打包发布文件"

# 构建目标
build: build-android build-relay
	@echo "✓ 构建完成"

build-android:
	@echo "构建 Android APK..."
	cd $(ANDROID_DIR) && ./gradlew assembleDebug
	@echo "✓ Android APK 构建完成: $(ANDROID_DIR)/app/build/outputs/apk/debug/app-debug.apk"

build-relay:
	@echo "构建 Rust 中继服务器..."
	cd $(RELAY_DIR) && cargo build --release
	@echo "✓ Rust 中继服务器构建完成: $(RELAY_DIR)/target/release/vortex"

build-android-release:
	@echo "构建 Android Release APK..."
	cd $(ANDROID_DIR) && ./gradlew assembleRelease
	@echo "✓ Android Release APK 构建完成: $(ANDROID_DIR)/app/build/outputs/apk/release/app-release.apk"

# 清理目标
clean:
	@echo "清理构建产物..."
	cd $(ANDROID_DIR) && ./gradlew clean
	cd $(RELAY_DIR) && cargo clean
	@echo "✓ 清理完成"

# 安装和运行
install: build-android
	@echo "安装 APK 到设备..."
	adb install -r $(ANDROID_DIR)/app/build/outputs/apk/debug/app-debug.apk
	@echo "✓ APK 安装完成"

run: build-relay
	@echo "启动中继服务器..."
	cd $(RELAY_DIR) && ./target/release/vortex relay

start:
	@echo "启动 VPN 服务..."
	adb shell am start -a com.vortex.android.START \
		-n com.vortex.android/.MainActivity

stop:
	@echo "停止 VPN 服务..."
	adb shell am start -a com.vortex.android.STOP \
		-n com.vortex.android/.MainActivity

# 测试
test: test-android test-relay
	@echo "✓ 所有测试完成"

test-android:
	@echo "运行 Android 测试..."
	cd $(ANDROID_DIR) && ./gradlew test

test-relay:
	@echo "运行 Rust 测试..."
	cd $(RELAY_DIR) && cargo test

# 代码质量
lint: lint-android lint-relay
	@echo "✓ 代码检查完成"

lint-android:
	@echo "检查 Android 代码..."
	cd $(ANDROID_DIR) && ./gradlew lint

lint-relay:
	@echo "检查 Rust 代码..."
	cd $(RELAY_DIR) && cargo clippy

format: format-android format-relay
	@echo "✓ 代码格式化完成"

format-android:
	@echo "格式化 Android 代码..."
	cd $(ANDROID_DIR) && ./gradlew spotlessApply || true

format-relay:
	@echo "格式化 Rust 代码..."
	cd $(RELAY_DIR) && cargo fmt

# 文档
docs:
	@echo "生成文档..."
	cd $(RELAY_DIR) && cargo doc --no-deps --open
	@echo "查看 Android 文档: $(ANDROID_DIR)/app/build/docs/javadoc/index.html"

# 发布
release: build-android-release build-relay
	@echo "准备发布..."
	mkdir -p $(ROOT_DIR)/release
	cp $(ANDROID_DIR)/app/build/outputs/apk/release/app-release.apk \
		$(ROOT_DIR)/release/vortex-android.apk
	cp $(RELAY_DIR)/target/release/vortex \
		$(ROOT_DIR)/release/vortex-server
	@echo "✓ 发布文件准备完成: $(ROOT_DIR)/release/"

package: release
	@echo "打包发布文件..."
	cd $(ROOT_DIR)/release && \
		zip -r vortex-linux-x64.zip vortex-server vortex-android.apk
	@echo "✓ 发布包创建完成: $(ROOT_DIR)/release/vortex-linux-x64.zip"

# ADB 相关
adb-devices:
	@echo "连接的设备:"
	@adb devices

adb-logcat:
	@echo "查看应用日志 (Ctrl+C 退出):"
	@adb logcat -s Vortex:V

adb-tunnel:
	@echo "设置 ADB 反向端口转发..."
	adb reverse localabstract:vortex tcp:31416

adb-tunnel-remove:
	@echo "移除 ADB 反向端口转发..."
	adb reverse --remove localabstract:vortex

# 开发辅助
watch-relay:
	@echo "监视 Rust 代码变化并自动重新编译..."
	@which cargo-watch > /dev/null || cargo install cargo-watch
	cd $(RELAY_DIR) && cargo watch -x build

benchmark:
	@echo "运行性能测试..."
	cd $(RELAY_DIR) && cargo bench

# 依赖检查
check-deps:
	@echo "检查依赖..."
	@echo "Rust 版本:"
	@rustc --version
	@cargo --version
	@echo ""
	@echo "Java 版本:"
	@java -version
	@echo ""
	@echo "ADB 版本:"
	@adb version
	@echo ""
	@echo "Android 设备:"
	@adb devices

# 初始化项目
init:
	@echo "初始化项目..."
	@echo "安装 Rust 依赖..."
	cd $(RELAY_DIR) && cargo fetch
	@echo "安装 Android 依赖..."
	cd $(ANDROID_DIR) && ./gradlew --refresh-dependencies
	@echo "✓ 项目初始化完成"
