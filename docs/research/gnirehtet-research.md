# Gnirehtet 深度研究报告与实现路线图

> **归档说明**：本文档是项目研究阶段的产物。部分内容（目录名、模块名、实现路线图）与最终实现有差异。如需了解当前架构，请参阅 [`docs/architecture.md`](../architecture.md)。

## 一、项目概述

### 1.1 核心功能
Gnirehtet 是一个 Android 反向网络代理工具，允许 Android 设备通过电脑的网络连接上网，无需 root 权限。

### 1.2 核心原理
- **VPN 拦截**: Android 端注册为 VPN 服务，拦截所有网络流量
- **ADB 反向转发**: 通过 `adb reverse` 建立设备到电脑的反向端口转发
- **中继服务器**: 电脑端接收原始 IPv4 包，解析并转发到真实网络

### 1.3 技术架构
```
┌─────────────────┐         ADB Reverse          ┌─────────────────┐
│  Android 设备   │  ◄──────────────────────────► │   电脑端       │
│                 │   localabstract:gnirehtet    │                 │
│  VPN Service    │         (TCP 连接)            │  Relay Server  │
│  (拦截流量)     │                                │  (处理包)      │
└─────────────────┘                                └─────────────────┘
```

## 二、核心组件分析

### 2.1 Android 端 (Java/Kotlin)

#### 关键类:
1. **GnirehtetService** - VPN 服务实现
   - 继承 `VpnService`
   - 配置 VPN 接口 (地址: 10.0.0.2, MTU: 16384)
   - 设置路由和 DNS 服务器

2. **Forwarder** - 数据转发器
   - 双向转发: 设备↔中继服务器
   - 使用线程池处理并发

3. **RelayTunnel** - 中继隧道
   - 通过 LocalSocket 连接到 ADB 转发的端口
   - 读写原始 IP 包

4. **IPPacketOutputStream** - IP 包输出流
   - 处理 TCP 流中的包边界问题

#### 核心流程:
```
1. 启动 VPN 服务
2. 建立 ADB 反向端口转发
3. 连接到中继服务器 (localhost:31416)
4. 接收客户端 ID
5. 双向转发 IP 包
```

### 2.2 电脑端中继服务器 (Rust)

#### 关键模块:

1. **Relay** - 主控制器
   - 创建 Selector (事件循环)
   - 启动 TunnelServer 监听端口 31416

2. **TunnelServer** - 隧道服务器
   - 接受客户端连接
   - 为每个客户端分配唯一 ID

3. **Client** - 客户端管理
   - 处理来自 Android 设备的原始 IP 包
   - 维护 Router 路由表

4. **Router** - 路由器
   - 根据五元组 (协议、源IP、源端口、目标IP、目标端口) 管理 Connection
   - 创建/查找 Connection

5. **Connection** - 连接抽象
   - **TcpConnection**: TCP 连接处理
   - **UdpConnection**: UDP 连接处理

6. **Packetizer** - 包构造器
   - 从应用层数据构造 IP 包
   - 添加 IP 头、TCP/UDP 头

#### TCP 连接状态机:
```
Init → SynSent → SynReceived → Established → CloseWait → LastAck
                      ↓
                FinWait1 → FinWait2
                      ↓
                   Closing
```

### 2.3 IP 包处理流程

#### 设备到网络:
```
Android App
    ↓ (VPN 拦截)
原始 IPv4 包
    ↓ (ADB Reverse)
Relay Server 接收
    ↓ (解析 IP 头)
Router 路由到 Connection
    ↓ (转换为应用层数据)
真实网络连接
```

#### 网络到设备:
```
真实网络响应
    ↓ (接收应用层数据)
Connection
    ↓ (Packetizer 构造 IP 包)
Relay Server
    ↓ (ADB Reverse)
Android VPN
    ↓ (注入到系统)
Android App
```

## 三、关键技术点

### 3.1 VPN 服务配置
```java
Builder builder = new Builder();
builder.addAddress("10.0.0.2", 32);
builder.addRoute("0.0.0.0", 0);  // 拦截所有流量
builder.addDnsServer("8.8.8.8"); // Google DNS
builder.setMtu(16384);           // 最大传输单元
builder.setBlocking(true);        // 同步 I/O
vpnInterface = builder.establish();
```

### 3.2 ADB 反向端口转发
```bash
adb reverse localabstract:gnirehtet tcp:31416
```
- 将设备的 `localabstract:gnirehtet` 映射到电脑的 `tcp:31416`
- Android 连接 `localhost` 实际连接到电脑的 31416 端口

### 3.3 IP 包解析
```rust
// IPv4 头结构 (20 字节)
struct IPv4Header {
    version: 4,              // 版本 (4位)
    ihl: 5,                  // 头长度 (4位)
    tos: 0,                  // 服务类型 (8位)
    total_length: u16,       // 总长度 (16位)
    identification: u16,     // 标识 (16位)
    flags: 3bits,            // 标志 (3位)
    fragment_offset: 13bits, // 片偏移 (13位)
    ttl: u8,                 // 生存时间 (8位)
    protocol: u8,            // 协议 (8位) - TCP(6)/UDP(17)
    checksum: u16,           // 校验和 (16位)
    source: [u8; 4],         // 源地址 (32位)
    destination: [u8; 4],    // 目标地址 (32位)
}

// TCP 头结构 (20 字节)
struct TCPHeader {
    source_port: u16,        // 源端口 (16位)
    dest_port: u16,          // 目标端口 (16位)
    seq_number: u32,         // 序列号 (32位)
    ack_number: u32,         // 确认号 (32位)
    data_offset: 4bits,      // 数据偏移 (4位)
    flags: 6bits,            // 标志位 (6位) - SYN/ACK/FIN/RST
    window: u16,             // 窗口大小 (16位)
    checksum: u16,           // 校验和 (16位)
    urgent_ptr: u16,         // 紧急指针 (16位)
}
```

### 3.4 TCP 状态管理
- 维护 TCB (Transmission Control Block)
- 处理 SYN/ACK/FIN/RST 标志
- 管理序列号和确认号
- 实现滑动窗口机制

### 3.5 异步 I/O 模型
- **Rust**: 使用 `mio` 库 (类似 epoll/kqueue)
- **Java**: 使用 NIO Selector
- 单线程事件循环，避免锁竞争

## 四、实现难点分析

### 4.1 TCP 连接可靠性
**问题**: 如何在不实现完整 TCP 协议栈的情况下保证可靠性？

**解决方案**:
1. 利用两端的 TCP 协议栈 (设备端 + 真实服务器端)
2. 只需保证网络→设备方向不丢包
3. 正确计算校验和
4. 遵守客户端窗口大小

### 4.2 IP 包边界问题
**问题**: TCP 是字节流，IP 包是数据报，如何区分边界？

**解决方案**:
- `IPPacketOutputStream`: 根据总长度字段切割 TCP 流
- 每次读取完整的 IP 包后再写入 VPN

### 4.3 校验和计算
**问题**: IP 包经过 NAT 后校验和失效

**解决方案**:
- IPv4 校验和: 只计算 IP 头
- TCP/UDP 校验和: 计算伪首部 + 头 + 数据
- 更新地址/端口后重新计算校验和

### 4.4 NAT 实现
**问题**: 多个设备连接同一个服务器如何区分？

**解决方案**:
- 使用五元组作为连接标识
- 维护连接映射表
- 类似端口受限锥形 NAT

## 五、从零实现路线图

### 阶段一：基础环境搭建 (1-2 周)

#### 任务清单:
- [ ] 安装开发环境
  - Android Studio + Android SDK
  - Rust 工具链 (cargo, rustup)
  - ADB 工具
- [ ] 创建项目结构
  ```
  vortex/
  ├── android-app/          # Android 客户端
  ├── relay-server/         # 中继服务器 (Rust)
  ├── docs/                 # 文档
  └── scripts/              # 构建脚本
  ```
- [ ] 学习基础知识
  - VPN API 使用
  - IP/TCP/UDP 协议
  - Rust 异步编程

### 阶段二：Android VPN 服务 (2-3 周)

#### 任务清单:
- [ ] 创建 Android 项目
- [ ] 实现 VpnService
  - 配置 VPN 接口
  - 设置路由和 DNS
  - 申请 VPN 权限
- [ ] 实现 VPN 数据读取
  - 从 FileDescriptor 读取 IP 包
  - 解析 IPv4 头
  - 过滤非 IPv4 包
- [ ] 实现 VPN 数据写入
  - 构造 IP 包
  - 写入 VPN 接口

#### 验证标准:
- [ ] VPN 服务能启动并拦截流量
- [ ] 能读取到 IP 包并打印日志
- [ ] 能将 IP 包写回系统

### 阶段三：ADB 通信 (1-2 周)

#### 任务清单:
- [ ] 实现 ADB 反向端口转发
  ```bash
  adb reverse localabstract:vortex tcp:31416
  ```
- [ ] Android 端连接到转发端口
  - 使用 LocalSocket
  - 建立 TCP 连接
- [ ] 实现简单的数据传输
  - 发送测试数据
  - 接收测试数据

#### 验证标准:
- [ ] Android 能连接到电脑端服务
- [ ] 双向数据传输正常

### 阶段四：中继服务器基础 (2-3 周)

#### 任务清单:
- [ ] 创建 Rust 项目
- [ ] 实现 TCP 服务器
  - 监听端口 31416
  - 接受客户端连接
  - 分配客户端 ID
- [ ] 实现异步 I/O
  - 使用 mio 库
  - 实现事件循环
  - 处理读写事件
- [ ] 实现包解析
  - 解析 IPv4 头
  - 解析 TCP/UDP 头
  - 提取五元组

#### 验证标准:
- [ ] 服务器能接受客户端连接
- [ ] 能解析接收到的 IP 包
- [ ] 日志输出包的详细信息

### 阶段五：UDP 连接实现 (2-3 周)

#### 任务清单:
- [ ] 实现 UdpConnection
  - 创建 UDP socket
  - 发送数据到目标
  - 接收响应数据
- [ ] 实现 Packetizer
  - 构造 IPv4 头
  - 构造 UDP 头
  - 计算校验和
- [ ] 实现 Router
  - 维护连接映射表
  - 根据五元组路由包
- [ ] 处理连接生命周期
  - 超时清理 (2分钟无活动)
  - 资源释放

#### 验证标准:
- [ ] DNS 查询能正常工作 (UDP 53 端口)
- [ ] 其他 UDP 应用能正常工作
- [ ] 连接能正确超时清理

### 阶段六：TCP 连接实现 (3-4 周)

#### 任务清单:
- [ ] 实现 TcpConnection
  - 连接到目标服务器
  - 状态机实现
  - 处理 SYN/ACK/FIN
- [ ] 实现序列号管理
  - 初始序列号 (随机)
  - 序列号递增
  - 窗口大小管理
- [ ] 实现数据转发
  - 设备→网络: 提取 TCP 载荷
  - 网络→设备: 封装为 IP 包
- [ ] 实现校验和计算
  - IP 头校验和
  - TCP 伪首部校验和

#### 验证标准:
- [ ] HTTP 请求能成功 (TCP 80 端口)
- [ ] HTTPS 请求能成功 (TCP 443 端口)
- [ ] 长连接能保持 (WebSocket)

### 阶段七：完整集成 (2-3 周)

#### 任务清单:
- [ ] Android 与服务器联调
  - 集成所有组件
  - 端到端测试
- [ ] 实现命令行工具
  - install: 安装 APK
  - run: 启动服务
  - stop: 停止服务
  - relay: 启动中继服务器
- [ ] 错误处理
  - 网络断开重连
  - 异常情况处理
  - 资源清理

#### 验证标准:
- [ ] 完整流程能跑通
- [ ] 浏览器能上网
- [ ] 各种应用能正常使用网络

### 阶段八：性能优化 (1-2 周)

#### 任务清单:
- [ ] 性能分析
  - 找出瓶颈
  - 内存使用分析
  - CPU 使用分析
- [ ] 优化措施
  - 减少内存拷贝
  - 优化包处理逻辑
  - 调整缓冲区大小
- [ ] 压力测试
  - 大流量测试
  - 长时间稳定性测试
  - 多设备并发测试

#### 验证标准:
- [ ] CPU 使用率 < 30% (满负载)
- [ ] 内存使用 < 100MB
- [ ] 连续运行 24 小时无崩溃

### 阶段九：跨平台支持 (2-3 周)

#### 任务清单:
- [ ] Windows 支持
  - 交叉编译
  - ADB 路径处理
  - 测试验证
- [ ] macOS 支持
  - 编译配置
  - 测试验证
- [ ] 打包发布
  - 构建脚本
  - 发布包制作
  - 文档编写

## 六、技术选型建议

### 6.1 语言选择
- **Android 端**: Java/Kotlin (必须)
- **服务端**:
  - 推荐使用 **Rust** (性能好、内存安全)
  - 备选: Go (开发效率高)、C++ (性能极致)

### 6.2 关键库
- **Android**: Android VPN API
- **Rust**: mio (异步 I/O)、byteorder (字节序)、rand (随机数)
- **网络**: 原生 socket API

### 6.3 开发工具
- **Android**: Android Studio
- **Rust**: VSCode + rust-analyzer
- **调试**: Wireshark (抓包分析)、tcpdump (Android 端)

## 七、学习资源

### 7.1 官方文档
- Android VPN API: https://developer.android.com/reference/android/net/VpnService
- IP 协议: RFC 791
- TCP 协议: RFC 793
- UDP 协议: RFC 768

### 7.2 参考资料
- Gnirehtet 源码: https://github.com/Genymobile/gnirehtet
- mio 库文档: https://docs.rs/mio
- Rust 网络编程: https://rust-lang.github.io/async-book/

### 7.3 调试技巧
```bash
# 抓取 Android 网络包
adb shell tcpdump -i any -w /sdcard/capture.pcap

# 查看 VPN 接口
adb shell ifconfig

# 监控系统日志
adb logcat -s Gnirehtet:V
```

## 八、常见问题与解决方案

### 8.1 VPN 权限被拒绝
**原因**: 用户未授权或系统限制
**解决**: 正确处理 onActivityResult，引导用户授权

### 8.2 连接断开后无法重连
**原因**: ADB 反向转发失效
**解决**: 监听设备插拔事件，重新建立转发

### 8.3 部分应用无法上网
**原因**: 应用使用了非标准协议或 IPv6
**解决**: 检查包过滤逻辑，考虑 IPv6 支持

### 8.4 性能问题
**原因**: 包处理逻辑复杂或内存拷贝过多
**解决**: 使用零拷贝技术，优化热点代码

## 九、扩展方向

### 9.1 功能增强
- IPv6 支持
- 流量统计
- 访问控制
- 多设备管理

### 9.2 性能优化
- 多线程处理
- 硬件加速
- 连接池

### 9.3 安全增强
- 流量加密
- 身份认证
- 访问日志

## 十、总结

Gnirehtet 的实现涉及多个技术领域:
- Android VPN 机制
- IP/TCP/UDP 协议
- 网络编程
- 异步 I/O
- 状态机设计

建议按照路线图分阶段实现，每个阶段都要充分测试验证。重点关注 TCP 连接的可靠性实现和性能优化。

预计总开发周期: **12-20 周** (根据经验水平和投入时间调整)
