use clap::Parser;
use vortex::cli::Cli;
use vortex::cli::Commands;
use vortex::adb::Adb;

fn main() {
    // 初始化日志——默认 info 级别，可通过 RUST_LOG 环境变量覆盖
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    env_logger::init();

    let cli = Cli::parse();
    let adb = Adb::new(cli.serial.clone());

    if let Err(e) = run_command(&cli.command, &adb) {
        eprintln!("错误: {e}");
        std::process::exit(1);
    }
}

/// 根据子命令执行对应操作。
fn run_command(command: &Commands, adb: &Adb) -> Result<(), String> {
    match command {
        Commands::Run => run_all(adb),
        Commands::Relay => run_relay(),
        Commands::Install => run_install(adb),
        Commands::Start => run_start(adb),
        Commands::Stop => run_stop(adb),
        Commands::Tunnel => adb.reverse(),
    }
}

/// `vortex run`：一键运行全部流程。
///
/// 顺序：安装 APK → 建隧道 → 启动 VPN → 启动 Relay
fn run_all(adb: &Adb) -> Result<(), String> {
    log::info!("=== Vortex 一键启动 ===");

    // 1. 安装 APK（从嵌入的二进制数据中提取）
    run_install(adb)?;

    // 2. 建立反向隧道
    adb.reverse()?;

    // 3. 启动 VPN
    adb.start_vpn()?;

    // 4. 启动 Relay 服务器
    run_relay()
}

/// 安装嵌入的 APK 到设备。
fn run_install(adb: &Adb) -> Result<(), String> {
    let apk_path = vortex::apk::extract().map_err(|e| e.to_string())?;
    adb.install(apk_path.to_str().ok_or("APK 路径无效")?)
}

/// `vortex start`：建隧道 + 启动 VPN。
fn run_start(adb: &Adb) -> Result<(), String> {
    adb.reverse()?;
    adb.start_vpn()
}

/// `vortex stop`：停止 VPN + 移除隧道。
fn run_stop(adb: &Adb) -> Result<(), String> {
    adb.stop_vpn()?;
    adb.reverse_remove()
}

/// `vortex relay`：启动中继服务器。
fn run_relay() -> Result<(), String> {
    log::info!("Relay 服务器启动中...");
    println!("Vortex Relay 正在运行，按 Ctrl+C 退出");
    vortex::relay::tunnel_server::Relay::run()
        .map_err(|e| format!("Relay 运行失败: {e}"))
}
