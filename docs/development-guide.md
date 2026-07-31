# Vortex 开发指南

## 一、环境搭建

### 1.1 Rust 工具链

```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# 验证
rustc --version
cargo --version
```

### 1.2 Android SDK

```bash
# macOS
brew install android-platform-tools

# 验证
adb version
```

或通过 [Android Studio](https://developer.android.com/studio) 安装完整 SDK。

### 1.3 设备准备

1. Android 设备开启 **开发者选项** → **USB 调试**
2. USB 连接设备到 PC
3. `adb devices` 确认设备已连接

## 二、构建

### 2.1 Relay Server（Rust）

```bash
cd server

# Debug 构建
cargo build

# Release 构建（LTO + opt-level=3）
cargo build --release

# 产物位置
# Debug:   server/target/debug/vortex
# Release: server/target/release/vortex
```

### 2.2 Android APK

```bash
cd vortex_app

# Debug APK
./gradlew assembleDebug

# Release APK（需要 keystore 配置）
./gradlew assembleRelease

# 产物位置
# Debug:   vortex_app/app/build/outputs/apk/debug/
# Release: vortex_app/app/build/outputs/apk/release/
```

## 三、运行

### 3.1 一键启动

```bash
cd server

# 安装 APK + 建隧道 + 启动 VPN + 启动 Relay
cargo run -- run
```

### 3.2 分步执行

```bash
cd server

cargo run -- install    # 安装 APK 到设备
cargo run -- tunnel     # 建立 ADB 反向隧道
cargo run -- start      # 建隧道 + 启动 VPN
cargo run -- relay      # 仅启动 Relay 服务器
cargo run -- stop       # 停止 VPN + 移除隧道
```

### 3.3 多设备

```bash
cargo run -- --serial <device_serial> run
```

## 四、测试

### 4.1 单元测试

```bash
cd server

# 运行所有单元测试（包含包解析、校验和、缓冲区等）
cargo test
```

### 4.2 集成测试

```bash
cd server

# TCP 端到端测试：SYN → SYN+ACK → ACK → 数据 → 回显
cargo test --test tcp_data_path

# UDP 端到端测试：发送包 → 回显 → 验证回传包
cargo test --test udp_data_path
```

### 4.3 Android 测试

```bash
cd vortex_app

# 单元测试
./gradlew test

# 设备测试
./gradlew connectedAndroidTest
```

## 五、代码结构导览

建议阅读顺序（自底向上）：

1. **packet/checksum.rs** — 校验和算法，最底层，无依赖
2. **packet/ipv4_header.rs** — IPv4 头零拷贝解析
3. **packet/tcp_header.rs, udp_header.rs** — 传输层头解析
4. **packet/transport_header.rs** — TCP/UDP 统一抽象
5. **packet/ipv4_packet.rs** — 分层访问 IP 头 + 传输层头 + payload
6. **packet/ipv4_packet_buffer.rs** — TCP 字节流 → IP 包边界恢复
7. **relay/stream_buffer.rs** — 简单字节流缓冲区
8. **relay/selector.rs** — 事件多路复用器，理解 EventHandler 闭包模式
9. **relay/connection.rs** — Connection trait + ConnectionId 五元组
10. **relay/packetizer.rs** — 回传 IP 包构造
11. **relay/packet_source.rs** — 背压 trait
12. **relay/client.rs** — Client + ClientChannel，理解借用拆分和脏标记
13. **relay/router.rs** — 五元组路由
14. **relay/udp_connection.rs** — UDP 转发（比 TCP 简单，先读这个）
15. **relay/tcp_connection.rs** — TCP 状态机（766 行，最复杂）
16. **relay/tunnel_server.rs** — 事件循环入口
17. **adb/mod.rs** — ADB 命令封装
18. **cli/args.rs** — 命令行定义
19. **main.rs** — 入口

## 六、调试技巧

### 6.1 Rust 日志

```bash
# 设置日志级别
RUST_LOG=debug cargo run -- relay

# 只看特定模块
RUST_LOG=vortex::relay::tcp_connection=trace cargo run -- relay

# 查看事件循环级别日志
RUST_LOG=vortex::relay::selector=trace cargo run -- relay
```

### 6.2 Android 日志

```bash
# 查看 VPN 服务日志
adb logcat -s VortexVpnService

# 查看 Relay 连接日志
adb logcat -s RelayConnection

# 查看所有 Vortex 相关日志
adb logcat | grep -i vortex
```

### 6.3 网络抓包

```bash
# 在 PC 上抓包
sudo tcpdump -i any -w vortex.pcap port 31416

# 用 Wireshark 分析
wireshark vortex.pcap
```

### 6.4 常见问题

| 问题 | 原因 | 解决 |
|------|------|------|
| `cargo run -- run` 报 "adb not found" | adb 不在 PATH | 安装 android-platform-tools |
| VPN 启动后无法上网 | ADB 隧道断开 | 重新插拔 USB，重新 `cargo run -- start` |
| 连接建立但无数据 | 防火墙阻止 | 检查 PC 防火墙是否放行 31416 端口 |
| TCP 连接卡住 | 模拟器地址未转换 | 确认 `10.0.2.2` → `127.0.0.1` 重写正常 |

## 七、编码规范

### Rust

- 遵循 [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- `///` 文档注释说明 "为什么"，`//` 行注释说明 "是什么"
- 错误处理：I/O 错误用 `io::Result`，逻辑错误用 `panic!` / `expect()` + 中文消息
- 日志：`log::info!` / `log::debug!` / `log::warn!` / `log::error!`，消息用中文

### Kotlin

- 遵循 [Kotlin Coding Conventions](https://kotlinlang.org/docs/coding-conventions.html)
- KDoc 注释公共 API
- Compose UI 组件用 `@Composable` 注解

### Git

- Conventional Commits：`feat(scope):` / `fix(scope):` / `docs:` / `test:` / `chore:`
- 原子提交：每个提交只做一件事，可独立 review
