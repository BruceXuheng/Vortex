# Vortex

<p align="center">
  <strong>零 Root 反向 USB 全局流量代理工具</strong>
</p>

<p align="center">
  让 Android 设备通过 ADB 隧道将全部网络流量完整代理到对端 PC，借用 PC 网络出口访问互联网。
</p>

<p align="center">
  <em>一线相连，流量入涡——你的手机，以 PC 之名上网。</em>
</p>

---

## 工作原理

```
┌─────────────────┐         ADB Reverse          ┌─────────────────┐
│  Android 设备   │  ◄──────────────────────────► │      PC        │
│                 │   localabstract:vortex        │                 │
│  VPN Service    │         (TCP 连接)            │  Relay Server  │
│  (拦截流量)     │                                │  (处理包)      │
└─────────────────┘                                └─────────────────┘
```

1. **VPN 拦截** — Android 端注册为 VPN 服务，拦截所有网络流量，获取原始 IPv4 包
2. **ADB 隧道** — 通过 `adb reverse` 建立设备到 PC 的反向端口转发（`localabstract:vortex` → `tcp:31416`）
3. **包转发** — PC 端 Relay Server 解析 IPv4/TCP/UDP 头，按五元组路由到真实网络连接
4. **网络访问** — 真实响应包经 Packetizer 构造回传 IP 包，原路返回 Android

## 项目结构

```
Vortex/
├── server/              # Rust 中继服务器
│   ├── src/
│   │   ├── main.rs      # CLI 入口（clap 子命令）
│   │   ├── lib.rs        # 库入口
│   │   ├── adb/          # ADB 命令封装
│   │   ├── cli/          # 命令行参数定义
│   │   ├── packet/       # IPv4/TCP/UDP 零拷贝解析
│   │   └── relay/        # 事件驱动中继核心
│   └── tests/            # TCP/UDP 集成测试
│
├── vortex_app/          # Android VPN 应用（Kotlin + Compose）
│   └── app/src/main/java/com/vortex/
│       ├── MainActivity.kt
│       ├── service/      # VpnService + PacketForwarder + RelayConnection
│       ├── data/         # TrafficLog 数据模型
│       └── ui/           # Compose UI（HomeScreen, LogDetail）
│
└── docs/                # 项目文档
```

## 快速开始

### 环境要求

- Rust 工具链（stable）
- Android SDK（`adb` 在 PATH 中）
- Android 设备已开启 USB 调试，通过 USB 连接 PC

### 构建

```bash
# 构建 Relay Server
cd server
cargo build --release

# 构建 Android APK
cd vortex_app
./gradlew assembleDebug
```

### 运行

```bash
cd server

# 一键启动：安装 APK + 建隧道 + 启动 VPN + 启动 Relay
cargo run -- run

# 或分步执行：
cargo run -- install    # 安装 APK 到设备
cargo run -- tunnel     # 建立 ADB 反向隧道
cargo run -- start      # 建隧道 + 启动 VPN
cargo run -- relay      # 仅启动 Relay 服务器
cargo run -- stop       # 停止 VPN + 移除隧道
```

### 多设备

```bash
# 通过 --serial 指定设备序列号
cargo run -- --serial <device_serial> run
```

## 命令参考

| 命令 | 说明 |
|------|------|
| `vortex run` | 一键运行：安装 APK + 建隧道 + 启动 VPN + 启动 Relay |
| `vortex relay` | 仅启动中继服务器 |
| `vortex install` | 安装 APK 到设备 |
| `vortex start` | 建立反向隧道 + 启动 VPN |
| `vortex stop` | 停止 VPN + 移除隧道 |
| `vortex tunnel` | 仅建立 ADB 反向隧道 |

## 架构概览

详见 [docs/architecture.md](docs/architecture.md)。

核心设计：
- **EventHandler 闭包模式** — 注册时捕获 `Rc<RefCell<Self>>`，事件触发时 `borrow_mut()` 调用 `on_ready()`
- **ClientChannel 借用拆分** — Connection 通过临时引用结构体回传数据，避免 borrow 冲突
- **PacketSource 背压** — Client 缓冲区满时，TcpConnection 注册为 pending source，空间后主动拉取
- **TCP 状态机** — Init → SynSent → SynReceived → Established → LastAck/FinWait，含延迟 FIN 和流控

## 开发

详见 [docs/development-guide.md](docs/development-guide.md)。

```bash
# 运行测试
cd server && cargo test

# 运行集成测试
cargo test --test tcp_data_path
cargo test --test udp_data_path

# 调试日志
RUST_LOG=debug cargo run -- relay
```

## 致谢

核心架构采用事件驱动 I/O + 内核级 VPN 拦截 + 反向端口转发三层代理模型。

## 许可证

[Apache-2.0](LICENSE)
