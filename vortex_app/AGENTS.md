# AGENTS.md — Vortex Android App

## 项目概述

Vortex 是一个**零 Root 反向 USB 全局流量代理工具**。Android 端通过 `VpnService` 拦截全部网络流量，原始 IPv4 包经 ADB 反向隧道转发到 PC 端 Relay Server，由后者通过真实网络发送。

核心数据流：`App → VPN fd → PacketForwarder → RelayConnection (LocalSocket) → ADB reverse → PC Relay Server (TCP :31416) → 真实网络`

## 技术栈

| 类别 | 技术 / 版本 |
|------|-------------|
| 语言 | Kotlin 2.2.10 |
| UI | Jetpack Compose + Material3 (BOM 2026.02.01) |
| 导航 | Navigation Compose 2.9.1 |
| 状态管理 | ViewModel + StateFlow |
| Service 通信 | BroadcastReceiver (Service → ViewModel) |
| 构建 | Gradle 9.4.1, AGP 9.2.1, JDK 21 toolchain |
| 最低 SDK | 26 (Android 8.0) |
| 目标 SDK | 36 |
| 编译 SDK | 37 |

## 目录结构

```
app/src/main/java/com/vortex/
├── MainActivity.kt              # 主 Activity，承载 NavHost
├── data/
│   └── TrafficLog.kt            # 流量日志数据类
├── service/
│   ├── VortexVpnService.kt     # VPN 前台服务：建立接口、管理生命周期、集成转发
│   ├── RelayConnection.kt      # ADB 反向隧道 LocalSocket 连接 + client_id 握手
│   └── PacketForwarder.kt      # 双线程 IP 包转发 + IPPacketOutputStream
└── ui/
    ├── navigation/
    │   └── VortexRoutes.kt     # 路由常量 (HOME, LOG_DETAIL_PAGE)
    ├── screens/
    │   ├── home/
    │   │   ├── HomeScreen.kt   # 首页：VPN 连接控制卡片
    │   │   └── VpnViewModel.kt # VPN 状态管理 + BroadcastReceiver
    │   └── logdetail/
    │       └── LogDetailScreen.kt # 日志详情页
    └── theme/
        ├── Color.kt            # 主题色值
        ├── Theme.kt            # Dynamic Color / 自定义主题
        └── Type.kt             # 字体排版
```

## 架构要点

### VPN 流量转发管道

```
┌──────────────────────────────────────────────────────┐
│ Android 设备                                         │
│                                                      │
│  App ──► VPN fd ──► PacketForwarder ──► RelayConn   │
│         (出站)       (设备→网络线程)     (LocalSocket) │
│                                                      │
│  App ◄── VPN fd ◄── PacketForwarder ◄── RelayConn   │
│         (入站)       (IPPacketOutputStream)           │
└──────────────────────────────────────────────────────┘
                         │
              adb reverse localabstract:vortex
                         │
              ┌──────────▼──────────┐
              │  PC Relay Server    │
              │  (TCP 127.0.0.1:31416) │
              └─────────────────────┘
```

- **RelayConnection**：通过 `LocalSocket("vortex", ABSTRACT)` 连接 ADB reverse 隧道，连接后读取 4 字节 big-endian `client_id`
- **PacketForwarder**：2 线程池，设备→网络方向直接转发完整 IP 包，网络→设备方向经 `IPPacketOutputStream` 按 IPv4 `total_length` 恢复包边界
- **IPPacketOutputStream**：内部类，从 TCP 字节流中切出完整 IP 包写入 VPN fd；缓冲区 2×65536 字节
- **wakeUpReadWorkaround**：VPN fd 的 `FileInputStream.read()` 不会因 close 唤醒，需发 UDP 空包到 `42.42.42.42:4242`

### Service ↔ ViewModel 通信

- ViewModel 通过 `startService(action)` 发送 `ACTION_START_VPN` / `ACTION_STOP_VPN`
- Service 通过 `sendBroadcast("com.vortex.VPN_STATE_CHANGED")` 广播状态 (`CONNECTED` / `DISCONNECTED` / `ERROR`)
- ViewModel 中注册 `BroadcastReceiver` 接收并更新 `StateFlow`

### 关键约束

- **不需要 `VpnService.protect()`**：`LocalSocket` 走 ADB USB 通道，不走系统网络栈，无路由回路风险
- **VPN 接口配置**：地址 `10.0.0.2/32`，路由 `0.0.0.0/0`，DNS `8.8.8.8`，MTU `16384`
- **ADB reverse namespace**：`vortex`，与 server 端 `adb.rs` 保持一致
- **Relay Server 端口**：`31416`，仅监听 `127.0.0.1`

## 构建与验证

```bash
# 编译 Debug APK
./gradlew assembleDebug

# 编译 Release APK（需 keystore）
./gradlew assembleRelease

# 运行单元测试
./gradlew test

# 运行设备端测试
./gradlew connectedAndroidTest
```

## 代码规范

- **所有公共 API 和关键逻辑必须有 KDoc 注释**
- 包名：`com.vortex`
- 日志 TAG 命名：`Vortex` 前缀，如 `VortexVpnService`、`VortexRelayConn`、`VortexForwarder`
- 异常处理：转发线程的 `IOException` 通过 `onError` 回调上报，不静默吞掉
- 线程：`ExecutorService` 固定线程池，通过 `Future.cancel(true)` 中断停止

## AndroidManifest 权限与服务

| 权限 / 组件 | 说明 |
|-------------|------|
| `INTERNET` | 网络通信 |
| `FOREGROUND_SERVICE` | VPN 前台服务 |
| `FOREGROUND_SERVICE_SPECIAL_USE` | Android 14+ 前台服务类型 |
| `VortexVpnService` | 绑定 VPN Service，`foregroundServiceType=specialUse`，需 `BIND_VPN_SERVICE` 权限 |

## CI/CD

GitHub Actions 工作流位于父仓库 `.github/workflows/build.yml`：
- **触发**：push main / `v*` tag / PR main
- **构建**：Debug APK（始终）+ Release APK（push 时，需 keystore secret）
- **发布**：`v*` tag 时自动发布 Release APK 到 GitHub Releases
