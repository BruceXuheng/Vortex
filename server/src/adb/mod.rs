use std::process::Command;

/// ADB 命令封装。
///
/// 所有 ADB 操作都通过调用系统 `adb` 完成。
/// 如果指定了 `serial`，所有命令会加上 `-s <serial>` 参数。
pub struct Adb {
    serial: Option<String>,
}

impl Adb {
    pub fn new(serial: Option<String>) -> Self {
        Self { serial }
    }

    /// 构建基础 `adb` 命令，自动附加 `-s serial`（如指定）。
    fn command(&self) -> Command {
        let mut cmd = Command::new("adb");
        if let Some(ref serial) = self.serial {
            cmd.arg("-s").arg(serial);
        }
        cmd
    }

    /// 安装 APK 到设备。
    ///
    /// 使用 `-r` 参数覆盖安装。
    pub fn install(&self, apk_path: &str) -> Result<(), String> {
        let status = self.command()
            .arg("install")
            .arg("-r")
            .arg(apk_path)
            .status()
            .map_err(|e| format!("执行 adb install 失败: {e}"))?;

        if status.success() {
            log::info!("APK 安装成功: {apk_path}");
            Ok(())
        } else {
            Err(format!("adb install 失败，退出码: {:?}", status.code()))
        }
    }

    /// 建立 ADB 反向隧道。
    ///
    /// 将设备上的 `localabstract:vortex` 映射到 PC 的 `tcp:31416`。
    /// 这样 Android 上通过 LocalSocket("vortex") 连接的数据，
    /// 会被 ADB 转发到 PC 的 31416 端口。
    pub fn reverse(&self) -> Result<(), String> {
        let status = self.command()
            .arg("reverse")
            .arg("localabstract:vortex")
            .arg("tcp:31416")
            .status()
            .map_err(|e| format!("执行 adb reverse 失败: {e}"))?;

        if status.success() {
            log::info!("反向隧道已建立: localabstract:vortex -> tcp:31416");
            Ok(())
        } else {
            Err(format!("adb reverse 失败，退出码: {:?}", status.code()))
        }
    }

    /// 移除反向隧道。
    pub fn reverse_remove(&self) -> Result<(), String> {
        let status = self.command()
            .arg("reverse")
            .arg("--remove")
            .arg("localabstract:vortex")
            .status()
            .map_err(|e| format!("执行 adb reverse --remove 失败: {e}"))?;

        if status.success() {
            log::info!("反向隧道已移除");
            Ok(())
        } else {
            Err(format!("adb reverse --remove 失败，退出码: {:?}", status.code()))
        }
    }

    /// 启动 VPN 服务。
    ///
    /// 通过 `am startservice` 发送 `com.vortex.action.START` Intent，
    /// 触发 VortexVpnService 建立 VPN 接口。
    pub fn start_vpn(&self) -> Result<(), String> {
        let status = self.command()
            .arg("shell")
            .arg("am")
            .arg("startservice")
            .arg("-a")
            .arg("com.vortex.action.START")
            .arg("com.vortex/.service.VortexVpnService")
            .status()
            .map_err(|e| format!("启动 VPN 失败: {e}"))?;

        if status.success() {
            log::info!("VPN 服务已启动");
            Ok(())
        } else {
            Err(format!("启动 VPN 失败，退出码: {:?}", status.code()))
        }
    }

    /// 停止 VPN 服务。
    pub fn stop_vpn(&self) -> Result<(), String> {
        let status = self.command()
            .arg("shell")
            .arg("am")
            .arg("startservice")
            .arg("-a")
            .arg("com.vortex.action.STOP")
            .arg("com.vortex/.service.VortexVpnService")
            .status()
            .map_err(|e| format!("停止 VPN 失败: {e}"))?;

        if status.success() {
            log::info!("VPN 服务已停止");
            Ok(())
        } else {
            Err(format!("停止 VPN 失败，退出码: {:?}", status.code()))
        }
    }
}
