use clap::{Parser, Subcommand};

/// Vortex — Android 反向 USB 全局流量代理工具
#[derive(Parser)]
#[command(name = "vortex", version, about = "一线相连，流量入涡")]
pub struct Cli {
    /// 指定 ADB 设备序列号（多设备时使用）
    #[arg(short, long, global = true)]
    pub serial: Option<String>,

    /// 子命令
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// 一键运行：安装 APK + 建隧道 + 启动 VPN + 启动 Relay
    Run {
        /// 自定义 DNS 服务器，逗号分隔（如 "8.8.8.8,1.1.1.1"）
        #[arg(short, long)]
        dns_servers: Option<String>,

        /// 仅代理指定路由，CIDR 逗号分隔（如 "192.168.0.0/16,10.0.0.0/8"）
        #[arg(short, long)]
        routes: Option<String>,
    },

    /// 仅启动中继服务器
    Relay,

    /// 安装 APK 到设备
    Install,

    /// 建立反向隧道 + 启动 VPN
    Start {
        /// 自定义 DNS 服务器，逗号分隔（如 "8.8.8.8,1.1.1.1"）
        #[arg(short, long)]
        dns_servers: Option<String>,

        /// 仅代理指定路由，CIDR 逗号分隔（如 "192.168.0.0/16,10.0.0.0/8"）
        #[arg(short, long)]
        routes: Option<String>,
    },

    /// 停止 VPN
    Stop,

    /// 仅建立 ADB 反向隧道
    Tunnel,
}
