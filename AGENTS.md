# AGENTS.md — Vortex Project

## 项目概述

Vortex 是一个**零 Root 反向 USB 全局流量代理工具**。Android 设备通过 `VpnService` 拦截全部网络流量，原始 IPv4 包经 ADB 反向隧道转发到 PC 端 Rust Relay Server，由后者解析并转发到真实网络，响应原路返回。

核心数据流：

```
App → VPN fd → PacketForwarder → LocalSocket → ADB reverse → PC Relay Server → 真实网络
```

## 仓库结构

```
Vortex/
├── server/              # Rust 中继服务器
│   ├── src/
│   │   ├── main.rs      # CLI 入口（clap 子命令）
│   │   ├── lib.rs        # pub mod cli, adb, packet, relay
│   │   ├── cli/          # 命令行参数（clap derive）
│   │   ├── adb/          # ADB 命令封装
│   │   ├── packet/       # IPv4/TCP/UDP 零拷贝解析 + 校验和
│   │   └── relay/        # 事件驱动中继核心
│   └── tests/            # TCP/UDP 集成测试
├── vortex_app/          # Android VPN 应用（Kotlin + Compose）
│   └── app/src/main/java/com/vortex/
│       ├── service/      # VpnService + PacketForwarder + RelayConnection
│       ├── data/         # TrafficLog
│       └── ui/           # Compose UI（HomeScreen, LogDetail）
├── docs/                # 项目文档
│   ├── architecture.md
│   ├── development-guide.md
│   └── research/        # 归档的研究资料
├── .github/workflows/   # CI（仅构建 Android APK）
└── LICENSE              # Apache-2.0
```

## 技术栈

| 层 | 技术 |
|-----|------|
| Server 语言 | Rust (edition 2021) |
| Server 异步 I/O | mio 0.8（边缘触发 + 手动 interest 更新） |
| Server CLI | clap 4 (derive) |
| Android 语言 | Kotlin 2.2.10 |
| Android UI | Jetpack Compose + Material3 |
| Android 构建 | Gradle 9.4.1, AGP 9.2.1, JDK 21 toolchain |
| Android SDK | minSdk 26, targetSdk 36, compileSdk 37 |

## Server 架构要点

### 模块依赖关系

```
main.rs → cli, adb, relay::tunnel_server
relay::tunnel_server → client, selector
client → packet::ipv4_packet, packet::ipv4_packet_buffer, router, selector, stream_buffer
router → connection, tcp_connection, udp_connection
tcp_connection → client::ClientChannel, packet_source, packetizer, selector, stream_buffer
udp_connection → client, connection, packetizer, selector
packetizer → checksum, ipv4_header
```

### 关键设计模式

1. **EventHandler 闭包模式** — `EventHandler` trait 签名为 `&self`，为 `Fn(&mut Selector, &Event)` 闭包自动实现。注册时捕获 `Rc<RefCell<Self>>`，闭包内 `borrow_mut()` 实现 `&mut self` 效果

2. **ClientChannel 借用拆分** — Connection 回传数据时，Router 已持有 Client 可变引用。ClientChannel 从 Client 提取 `&mut StreamBuffer` + `&mut Interest`，让 Connection 安全写入，避免 borrow 冲突

3. **脏标记 interest 更新** — `mark_interests_update()` 将 `interests` 强制设为 `READABLE`（故意错误值），下次 `update_interests()` 计算正确值时检测不匹配，触发 `reregister`

4. **PacketSource 背压** — Client 缓冲区满时，TcpConnection 注册为 pending packet source，Client 空间后 `process_pending()` 主动拉取

5. **TCP 延迟 FIN** — 收到 FIN 只设 `fin_received = true`，等 `client_to_network` 缓冲区清空后再处理，防止丢数据

6. **延迟 deregister** — `Selector::deregister()` 立即从 mio Poll 移除 source，但 handler 在 `run_handlers()` 结束后才清理，避免迭代中修改 Slab

7. **Weak\<RefCell\<Client\>\>** — TcpConnection/UdpConnection 持有弱引用，Client 关闭后自动失效，避免循环引用

### 关键常量

| 常量 | 值 | 位置 |
|------|-----|------|
| Relay 端口 | 31416 | `tunnel_server.rs` |
| ADB namespace | `vortex` | `adb/mod.rs` |
| VPN 地址 | 10.0.0.2/32 | `VortexVpnService.kt` |
| VPN MTU | 16384 | `VortexVpnService.kt` |
| TCP MTU | 0x4000 (16384) | `tcp_connection.rs` |
| UDP 空闲超时 | 120s | `udp_connection.rs` |

### Connection trait 设计

`send_to_network()` 不接收 ClientChannel——Connection 只解析包和更新状态。回传数据走独立的事件驱动路径（socket READABLE → process_receive → ClientChannel）。这避免 Router 同时持有 Client 和 Connection 可变引用的 borrow 冲突。

## Android App 架构要点

详见 `vortex_app/AGENTS.md`。

- **VortexVpnService** — VPN 前台服务，配置 VPN 接口，管理 PacketForwarder 生命周期
- **RelayConnection** — `LocalSocket("vortex", ABSTRACT)` + 4 字节 client_id 握手
- **PacketForwarder** — 双线程转发，内置 `IPPacketOutputStream` 按 `total_length` 恢复 IP 包边界
- **不需要 `VpnService.protect()`** — LocalSocket 走 ADB USB 通道，不走系统网络栈

## 构建与验证

```bash
# Server
cd server
cargo build --release
cargo test                              # 15 单元测试
cargo test --test tcp_data_path         # TCP 集成测试
cargo test --test udp_data_path         # UDP 集成测试

# Android
cd vortex_app
./gradlew assembleDebug
./gradlew test
```

## 运行

```bash
cd server
cargo run -- run          # 一键：安装 APK + 建隧道 + 启动 VPN + 启动 Relay
cargo run -- relay        # 仅启动 Relay 服务器
cargo run -- start        # 建隧道 + 启动 VPN
cargo run -- stop         # 停止 VPN + 移除隧道
cargo run -- --serial <id> run   # 多设备指定序列号
```

## 代码规范

### Rust (server/)

- 遵循 Rust API Guidelines
- `///` 文档注释说"为什么"，`//` 行注释说"是什么"
- I/O 错误用 `io::Result`，逻辑错误用 `panic!`/`expect()` + 中文消息
- 日志消息用中文：`log::info!` / `log::debug!` / `log::warn!` / `log::error!`
- 修改代码前先 Read 目标文件

### Kotlin (vortex_app/)

- 所有公共 API 和关键逻辑必须有 KDoc 注释
- 日志 TAG：`Vortex` 前缀
- 异常：转发线程 `IOException` 通过 `onError` 回调上报

### Git

- Conventional Commits：`feat(scope):` / `fix(scope):` / `docs:` / `test:` / `chore:`
- 原子提交：每个提交只做一件事

## CI/CD

GitHub Actions（`.github/workflows/build.yml`）：
- **触发**：push main / `v*` tag / PR main
- **构建**：Debug APK（始终）+ Release APK（push 时）
- **发布**：`v*` tag 时自动发布到 GitHub Releases
- **注意**：CI 仅构建 Android APK，无 Rust Server 构建流程
