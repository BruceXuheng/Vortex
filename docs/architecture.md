# Vortex 架构设计

## 一、项目概述

Vortex 是一个零 Root Android 反向 USB 全局流量代理工具。Android 设备通过 VPN 拦截所有网络流量，将原始 IPv4 包经由 ADB 反向隧道发送到 PC 端的 Rust Relay Server，由后者解析并转发到真实网络，响应原路返回。

核心架构基于**事件驱动 I/O + 内核级 VPN 拦截 + 反向端口转发**三层代理模型，Rust 实现确保零开销抽象和内存安全。

## 二、目录结构

```
Vortex/
├── server/                          # Rust 中继服务器
│   ├── Cargo.toml                   # name = "vortex", edition = 2021
│   ├── src/
│   │   ├── main.rs                  # CLI 入口（clap 子命令）
│   │   ├── lib.rs                   # pub mod cli, adb, packet, relay
│   │   ├── cli/
│   │   │   ├── mod.rs               # re-export Cli, Commands
│   │   │   └── args.rs              # clap Parser/Subcommand 定义
│   │   ├── adb/
│   │   │   └── mod.rs               # ADB 命令封装（install, reverse, start_vpn 等）
│   │   ├── packet/
│   │   │   ├── mod.rs
│   │   │   ├── checksum.rs          # IPv4/TCP/UDP 校验和（RFC 1071）
│   │   │   ├── ipv4_header.rs       # 零拷贝 Ipv4Header 读 + Ipv4HeaderMut 写
│   │   │   ├── ipv4_packet.rs       # Ipv4Packet 分层访问：IP 头 → 传输层头 → payload
│   │   │   ├── ipv4_packet_buffer.rs# TCP 字节流 → IP 包边界解析器
│   │   │   ├── tcp_header.rs        # 零拷贝 TcpHeader 读 + TcpHeaderMut 写
│   │   │   ├── transport_header.rs  # TransportHeader 枚举统一 TCP/UDP
│   │   │   └── udp_header.rs        # 零拷贝 UdpHeader 读 + UdpHeaderMut 写
│   │   └── relay/
│   │       ├── mod.rs
│   │       ├── tunnel_server.rs     # TunnelServer 监听 + Relay 事件循环
│   │       ├── selector.rs          # mio Poll + Slab<Rc<dyn EventHandler>>
│   │       ├── client.rs            # Client + ClientChannel
│   │       ├── connection.rs        # Connection trait + ConnectionId 五元组
│   │       ├── router.rs            # 五元组路由器
│   │       ├── packetizer.rs        # 回传 IP 包构造器
│   │       ├── packet_source.rs     # PacketSource trait（背压恢复）
│   │       ├── stream_buffer.rs     # 字节流缓冲区
│   │       ├── tcp_connection.rs    # TCP 连接状态机 + 流控
│   │       └── udp_connection.rs    # UDP 连接转发
│   └── tests/
│       ├── tcp_data_path.rs         # TCP 端到端集成测试
│       └── udp_data_path.rs         # UDP 端到端集成测试
│
├── vortex_app/                      # Android VPN 应用（Kotlin + Compose）
│   ├── app/src/main/java/com/vortex/
│   │   ├── MainActivity.kt          # Compose 主界面入口
│   │   ├── data/TrafficLog.kt       # 流量日志数据模型
│   │   ├── service/
│   │   │   ├── VortexVpnService.kt  # VPN 服务核心
│   │   │   ├── RelayConnection.kt   # LocalSocket 连接 + client_id 握手
│   │   │   └── PacketForwarder.kt   # 双向转发 + IP 包边界恢复
│   │   └── ui/
│   │       ├── navigation/VortexRoutes.kt
│   │       ├── screens/home/HomeScreen.kt, VpnViewModel.kt
│   │       ├── screens/logdetail/LogDetailScreen.kt
│   │       └── theme/Color.kt, Theme.kt, Type.kt
│   └── build.gradle.kts             # Kotlin DSL, Compose, minSdk 26
│
└── docs/                            # 项目文档
```

## 三、数据流详解

### 3.1 出站（Android → Internet）

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  Android App │     │  VPN Service │     │ LocalSocket  │
│  (发起请求)  │ ──► │ (拦截 IP 包) │ ──► │ (通过 ADB)   │
└──────────────┘     └──────────────┘     └──────┬───────┘
                                                  │ ADB reverse
                                                  │ localabstract:vortex → tcp:31416
                                                  ▼
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  Connection  │     │    Router    │     │ Relay Server │
│ (TCP/UDP)    │ ◄── │ (五元组路由) │ ◄── │ (接收 IP 包) │
└──────┬───────┘     └──────────────┘     └──────────────┘
       │
       ▼
┌──────────────┐
│ 真实服务器   │
│ (应用层数据) │
└──────────────┘
```

1. App 发起网络请求
2. `VortexVpnService` 通过 VPN 接口拦截，获取完整 IPv4 包（地址 `10.0.0.2/32`，路由 `0.0.0.0/0`，MTU 16384）
3. `PacketForwarder` 从 VPN fd 读取完整 IP 包，通过 `RelayConnection` 的 `LocalSocket("vortex")` 发出
4. ADB 反向隧道将数据转发到 PC 的 `127.0.0.1:31416`
5. `TunnelServer` 接受连接，发送 4 字节 `client_id`（big-endian）
6. `Client.process_receive()` 通过 `Ipv4PacketBuffer` 从 TCP 字节流中恢复 IP 包边界
7. `Router.send_to_network()` 按五元组查找或创建 `Connection`
8. `Connection.send_to_network()` 处理包：
   - **TCP**：`TcpConnection` 解析 TCP 标志，更新 TCB 状态，payload 写入 `client_to_network` 缓冲区
   - **UDP**：`UdpConnection` 直接将 payload 发送到真实 UDP socket

### 3.2 入站（Internet → Android）

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│ 真实服务器   │     │  Connection  │     │  Packetizer  │
│ (响应数据)   │ ──► │ (接收响应)   │ ──► │ (构造 IP 包) │
└──────────────┘     └──────────────┘     └──────┬───────┘
                                                  │ ClientChannel.send_to_client()
                                                  ▼
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  Android App │     │  VPN Service │     │ LocalSocket  │
│ (接收响应)   │ ◄── │ (注入系统)   │ ◄── │ (通过 ADB)   │
└──────────────┘     └──────────────┘     └──────────────┘
```

1. 真实网络响应到达 TCP/UDP socket
2. `Selector` 分发 READABLE 事件到 Connection 的 handler
3. `TcpConnection.process_receive()` / `UdpConnection.process_receive()` 从真实 socket 读取数据
4. `Packetizer` 从参考包（Android 发来的原始包）复制头部，交换 src/dst IP 和端口，写入新 payload，重算校验和
5. 通过 `ClientChannel.send_to_client()` 写入 Client 的 `network_to_client` 缓冲区
6. `Client.process_send()` 将缓冲区数据写入 TCP 流，经 ADB 隧道发回 Android
7. Android 端 `PacketForwarder` 通过 `IPPacketOutputStream` 恢复 IP 包边界（读 IPv4 `total_length` 字段）
8. 完整 IP 包写入 VPN fd，Android 网络栈处理并投递给 App

### 3.3 地址重写

`ConnectionId.rewritten_destination()` 将 `10.0.2.2`（Android 模拟器中表示宿主机的特殊地址）转换为 `127.0.0.1`。这是唯一的地址重写规则，其他目的地址直接透传。

## 四、Server 模块详解

### 4.1 packet 层——零拷贝解析

所有头部解析器都基于原始字节切片，不拷贝数据：

- **`Ipv4Header` / `Ipv4HeaderMut`** — 读取/修改 IP 头字段（version、IHL、total_length、protocol、src/dst IP），通过校验和计算验证完整性
- **`TcpHeader` / `TcpHeaderMut`** — 读取/修改 TCP 头字段（port、seq、ack、flags、window），提供 `is_syn()`/`is_ack()`/`is_fin()` 便捷方法
- **`UdpHeader` / `UdpHeaderMut`** — 读取/修改 UDP 头字段，校验和设为 0 禁用
- **`TransportHeader`** — `enum TransportHeader { Tcp(TcpHeader), Udp(UdpHeader) }` 统一抽象
- **`Ipv4Packet`** — 分层访问：`ipv4_header()` → `transport_header()` → `transport_payload()`
- **`Ipv4PacketBuffer`** — 从 TCP 字节流中按 `total_length` 恢复 IP 包边界，支持增量读取和部分包缓存
- **`checksum`** — 实现 RFC 1071 校验和算法，IPv4 头校验和 + TCP/UDP 伪首部校验和

### 4.2 relay 层——事件驱动架构

#### Selector（事件多路复用器）

```
Selector {
    poll: mio::Poll,
    handlers: Slab<Rc<dyn EventHandler>>,
    tokens_to_remove: Vec<Token>,
}
```

封装 mio 0.8 的 `Poll`，用 `Slab` 管理 Token → Handler 映射。关键设计：

- **EventHandler 闭包模式**：`EventHandler` 是 `&self` 签名的 trait，为 `Fn(&mut Selector, &Event)` 闭包自动实现。注册时捕获 `Rc<RefCell<Self>>`，闭包内调用 `borrow_mut()` 实现 `&mut self` 效果
- **延迟 deregister**：`deregister()` 立即从 mio Poll 移除 source，但 handler 在 `run_handlers()` 结束后才清理，避免迭代中修改 Slab
- **没有 defer_reregister**：handler 在 `on_ready()` 结束后自行调用 `update_interests()`

#### Client + ClientChannel

```
Client {
    stream: TcpStream,              // 与 Android 的 TCP 连接
    interests: Interest,            // 当前注册的 interest（跟踪状态避免不必要 reregister）
    client_to_network: Ipv4PacketBuffer,  // 从设备读入的 IP 包
    network_to_client: StreamBuffer,      // 待发送到设备的 IP 包
    router: Router,                 // 五元组路由器
    pending_packet_sources: Vec<Rc<RefCell<dyn PacketSource>>>,  // 背压
    pending_id_bytes: usize,        // 初始阶段：还需要发送的 client_id 字节数
}

ClientChannel<'a> {
    network_to_client: &'a mut StreamBuffer,
    token: Token,
    interests: &'a mut Interest,
}
```

- **ClientChannel 借用拆分**：Connection 回传数据时，Router 已经借用了 Client。ClientChannel 从 Client 中提取 `network_to_client` buffer 和 `interests` 字段的可变引用，让 Connection 可以安全写入
- **脏标记机制**：`mark_interests_update()` 将 `interests` 强制设为 `READABLE`（一个故意错误的值），下次 `update_interests()` 计算出正确值时会检测到不匹配，触发 `reregister`
- **先写后读**：`process()` 中先 `process_send()` 再 `process_receive()`，因为发送可能腾出缓冲区空间
- **`update_interests()`**：buffer 为空 → `READABLE`，buffer 非空 → `READABLE | WRITABLE`，只在 interest 变化时调用 reregister

#### Router（五元组路由器）

```
Router {
    client: Weak<RefCell<Client>>,
    connections: Vec<Rc<RefCell<dyn Connection>>>,
}
```

- 持有 `Weak<RefCell<Client>>`，不持有关闭监听器
- `send_to_network()` 不传 ClientChannel——Connection 只解析包和更新状态，回传数据通过 on_ready 事件路径完成
- Connection 关闭时通过 `is_closed()` 标记，Router 在下次访问时 `swap_remove` 清理

#### Connection trait

```rust
pub trait Connection {
    fn id(&self) -> ConnectionId;
    fn send_to_network(&mut self, selector: &mut Selector, ipv4_packet: &[u8]);
    fn close(&mut self, selector: &mut Selector);
    fn is_expired(&self) -> bool;
    fn is_closed(&self) -> bool;
}
```

关键设计：`send_to_network()` 不接收 ClientChannel。Connection 只解析包和更新内部状态，回传数据在事件驱动路径中完成（Connection socket 收到 READABLE 事件 → `process_receive()` 读取网络数据 → 构造 IP 包 → 通过 `ClientChannel` 回传）。这避免了 Router 同时持有 Client 可变引用和 Connection 可变引用的借用冲突。

#### TcpConnection（TCP 连接状态机）

766 行，最核心的模块。实现完整的 TCB（Transmission Control Block）状态机：

```
Init → SynSent → SynReceived → Established
                                      │
                     ┌────────────────┤
                     ▼                ▼
               LastAck          FinWait1
                     │                │
                     ▼                ▼
                  (closed)        FinWait2/Closing
```

关键特性：
- **流控**：`remaining_client_window()` = `their_ack + client_window - seq`，窗口为 0 时停止读取
- **延迟 FIN**：收到 FIN 只设 `fin_received = true`，等 `client_to_network` 缓冲区清空后再处理
- **跳过 CloseWait**：Established 收到 FIN 后直接到 LastAck（发送 FIN+ACK 后等 ACK 关闭）
- **PacketSource 背压**：缓冲区满时将自己注册为 pending packet source
- **`may_read()` / `may_write()`**：动态计算 interest

#### UdpConnection（UDP 连接转发）

简单转发：收到 Android 的 UDP 包直接转发到真实 socket，收到真实响应后通过 Packetizer 构造回传包。120 秒空闲超时自动关闭。

#### Packetizer（IP 包构造器）

缓存 65536 字节 buffer，从参考包复制头部模板，交换 src/dst IP 和端口，写入新 payload，重算校验和。TCP 包额外缩减选项到 20 字节并计算伪首部校验和。

### 4.3 cli / adb 层

- **cli**：基于 `clap derive` 定义 6 个子命令（`run`/`relay`/`install`/`start`/`stop`/`tunnel`），支持 `--serial` 指定设备
- **adb**：封装系统 `adb` 命令，自动附加 `-s serial`。核心操作：`install -r`、`reverse localabstract:vortex tcp:31416`、`am startservice` 启停 VPN

## 五、Android App 架构

- **VortexVpnService** — 继承 `VpnService`，配置 VPN 接口（`10.0.0.2/32`, `0.0.0.0/0`, DNS `8.8.8.8`, MTU 16384），管理 `PacketForwarder` 生命周期，通过广播通知 UI 状态
- **RelayConnection** — `LocalSocket("vortex", ABSTRACT)` 连接到 ADB 反向隧道，先读 4 字节 `client_id`，然后双向转发
- **PacketForwarder** — 两个线程分别处理设备→网络和网络→设备方向的 IP 包转发，内置 `IPPacketOutputStream` 按 `total_length` 恢复 IP 包边界
- **UI** — Jetpack Compose + Material3，`HomeScreen` 显示 VPN 状态，`LogDetailScreen` 显示流量日志，`VpnViewModel` 管理状态

## 六、关键设计决策

### 为什么用 EventHandler 闭包而不是 trait object 方法？

mio 0.8 的 `Registry::register()` 需要 `&mut impl Source`，注册时必须拿到 source 的可变引用。闭包模式让每个组件在注册时捕获 `Rc<RefCell<Self>>`，事件触发时 `borrow_mut()` 获得 `&mut self`，比 `&self` + `self_ref` 的模式更简洁。

### 为什么 ClientChannel 不持有 stream 引用？

ClientChannel 是临时借用结构体，从 Client 中提取 buffer 和 interests 的可变引用。它不持有 stream 引用，不能直接调用 reregister——而是通过修改 `interests` 字段标记需要更新，Client 在 `process()` 结束后统一执行 reregister。这避免了在 borrow 拆分期间进行系统调用。

### 为什么 Connection.send_to_network() 不传 ClientChannel？

如果 Router 调用 `connection.send_to_network(selector, &mut channel, packet)`，Router 就同时持有了 Client 的可变引用（通过 channel）和 Connection 的可变引用。这本身不是问题，但会使得 Router 的 borrow 链更复杂。当前设计让 Connection 只做"解析 + 更新状态"，回传数据走独立的事件驱动路径，职责更清晰。

### 为什么用 Weak\<RefCell\<Client\>\>？

TcpConnection 和 UdpConnection 持有 `Weak<RefCell<Client>>` 而非 `Rc`。如果 Client 关闭但 Connection 还在，弱引用会返回 `None`，避免循环引用导致的内存泄漏。

## 七、关键配置常量

| 常量 | 值 | 位置 |
|------|-----|------|
| Relay 端口 | 31416 | `tunnel_server.rs` RELAY_PORT |
| ADB 命名空间 | `vortex` | `adb/mod.rs`, `RelayConnection.kt` |
| VPN 地址 | 10.0.0.2/32 | `VortexVpnService.kt` |
| VPN 路由 | 0.0.0.0/0 | `VortexVpnService.kt` |
| VPN DNS | 8.8.8.8 | `VortexVpnService.kt` |
| VPN MTU | 16384 | `VortexVpnService.kt` |
| TCP MTU | 0x4000 (16384) | `tcp_connection.rs` MTU |
| UDP 空闲超时 | 120s | `udp_connection.rs` UDP_IDLE_TIMEOUT_SECS |
| 连接清理间隔 | 60s | `tunnel_server.rs` CLEANUP_INTERVAL_SECS |

## 八、依赖

```toml
# server/Cargo.toml
[dependencies]
mio = { version = "0.8", features = ["os-poll", "net"] }  # 非阻塞 I/O
slab = "0.4"                                                # Token → Handler 映射
log = "0.4"                                                 # 日志门面
env_logger = "0.11"                                         # 日志实现
clap = { version = "4", features = ["derive"] }             # CLI
rand = "0.8"                                                # TCP ISN 生成

[profile.release]
opt-level = 3
lto = true
```
