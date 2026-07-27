# Vortex

<p align="center">
  <strong>零 Root 反向 USB 全局流量代理工具</strong>
</p>

<p align="center">
  让 Android 设备通过 ADB 隧道将全部网络流量完整代理到对端 PC，借用 PC 网络出口访问互联网。
</p>

<p align="center">
  <em>Slogan: 一线相连，流量入涡——你的手机，以 PC 之名上网。</em>
</p>

<p align="center">
  <a href="#特性">特性</a> •
  <a href="#快速开始">快速开始</a> •
  <a href="#文档">文档</a> •
  <a href="#开发">开发</a> •
  <a href="#路线图">路线图</a>
</p>

---

## 特性

- ✅ **零 Root 权限** - 无需对 Android 设备或 PC 进行 root
- ✅ **全局代理** - 拦截设备所有网络流量
- ✅ **多协议支持** - 支持 TCP 和 UDP 协议
- ✅ **跨平台** - 支持 Linux、Windows、macOS
- ✅ **高性能** - 基于 Rust 实现，低 CPU 和内存占用
- ✅ **简单易用** - 一键启动，无需复杂配置

## 工作原理

```
┌─────────────────┐         ADB Reverse          ┌─────────────────┐
│  Android 设备   │  ◄──────────────────────────► │      PC        │
│                 │   localabstract:vortex        │                 │
│  VPN Service    │         (TCP 连接)            │  Relay Server  │
│  (拦截流量)     │                                │  (处理包)      │
└─────────────────┘                                └─────────────────┘
```

1. **VPN 拦截**: Android 端注册为 VPN 服务，拦截所有网络流量
2. **ADB 隧道**: 通过 `adb reverse` 建立设备到 PC 的反向端口转发
3. **包转发**: 将原始 IPv4 包从设备发送到 PC 的中继服务器
4. **网络访问**: 中继服务器解析包并通过真实网络发送

## 快速开始

### 环境要求

- Android 5.0+ (API 21+)
- Android SDK Platform Tools (adb)
- PC: Windows / Linux / macOS
 
