use std::process::Command;

/// Vortex Android 应用的基础包名。
///
/// debug 构建会添加 `.debug` 后缀（由 Gradle `applicationIdSuffix` 控制），
/// release 构建使用基础包名。
const PACKAGE_BASE: &str = "com.vortex";

/// Activity 和 Service 的实际类名全限定路径。
///
/// 注意：applicationId 只改变 Manifest 中的包标识，不改变 Java/Kotlin 类的包名。
/// 因此 `.MainActivity` 缩写在 debug 包下会展开为 `com.vortex.debug.MainActivity`（错误），
/// 必须使用完整类名 `com.vortex.MainActivity`。
const ACTIVITY_CLASS: &str = "com.vortex.MainActivity";

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

    /// 检测设备上安装的 Vortex 包名。
    ///
    /// 优先匹配 release 包名 `com.vortex`，其次匹配 debug 包名 `com.vortex.debug`。
    /// 如果都未安装，返回 release 包名（后续操作会报错）。
    fn detect_package(&self) -> String {
        let output = self.command()
            .arg("shell")
            .arg("pm")
            .arg("list")
            .arg("packages")
            .output();

        if let Ok(out) = output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            // 优先匹配 release 包
            if stdout.contains(&format!("package:{PACKAGE_BASE}\n")) {
                return PACKAGE_BASE.to_string();
            }
            // 其次匹配 debug 包
            if stdout.contains(&format!("package:{PACKAGE_BASE}.debug\n")) {
                return format!("{PACKAGE_BASE}.debug");
            }
        }

        // 默认返回 release 包名
        PACKAGE_BASE.to_string()
    }

    /// 安装 APK 到设备。
    ///
    /// 使用 `-r -g -d` 参数覆盖安装、授予权限、允许降级。
    pub fn install(&self, apk_path: &str) -> Result<(), String> {
        let status = self.command()
            .arg("install")
            .arg("-r")
            .arg("-g")
            .arg("-d")
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
    /// 通过 `am start` 发送 `com.vortex.action.START` Intent 到 Activity，
    /// 由 Activity 内部启动 VortexVpnService。
    ///
    /// 可选参数通过 `--esa`（extended string array）传递：
    /// - `dns_servers`：逗号分隔的 DNS 服务器列表
    /// - `routes`：逗号分隔的 CIDR 路由规则列表
    ///
    /// 不能直接用 `am startservice` 启动 VpnService：
    /// VpnService 声明了 `BIND_VPN_SERVICE` 系统权限保护，
    /// ADB shell 没有该权限，跨 UID 调用会被拒绝。
    pub fn start_vpn(
        &self,
        dns_servers: Option<&str>,
        routes: Option<&str>,
    ) -> Result<(), String> {
        let package = self.detect_package();
        let component = format!("{package}/{ACTIVITY_CLASS}");

        let mut cmd = self.command();
        cmd.arg("shell")
            .arg("am")
            .arg("start")
            .arg("-a")
            .arg("com.vortex.action.START")
            .arg("-n")
            .arg(&component);

        // 传递 DNS 服务器列表（--esa dnsServers 8.8.8.8,1.1.1.1）
        if let Some(dns) = dns_servers {
            cmd.arg("--esa").arg("dnsServers").arg(dns);
        }
        // 传递路由规则列表（--esa routes 192.168.0.0/16,10.0.0.0/8）
        if let Some(rt) = routes {
            cmd.arg("--esa").arg("routes").arg(rt);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("启动 VPN 失败: {e}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            log::info!("VPN 启动命令已发送 (package={package})");
            if !stdout.trim().is_empty() {
                log::info!("am start 输出: {}", stdout.trim());
            }
            Ok(())
        } else {
            Err(format!(
                "启动 VPN 失败，退出码: {:?}\nstdout: {}\nstderr: {}",
                output.status.code(),
                stdout.trim(),
                stderr.trim()
            ))
        }
    }

    /// 停止 VPN 服务。
    ///
    /// 通过 `am start` 发送 `com.vortex.action.STOP` Intent 到 Activity，
    /// 由 Activity 内部停止 VortexVpnService。
    pub fn stop_vpn(&self) -> Result<(), String> {
        let package = self.detect_package();
        let component = format!("{package}/{ACTIVITY_CLASS}");

        let output = self.command()
            .arg("shell")
            .arg("am")
            .arg("start")
            .arg("-a")
            .arg("com.vortex.action.STOP")
            .arg("-n")
            .arg(&component)
            .output()
            .map_err(|e| format!("停止 VPN 失败: {e}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        if output.status.success() {
            log::info!("VPN 停止命令已发送 (package={package})");
            if !stdout.trim().is_empty() {
                log::info!("am start 输出: {}", stdout.trim());
            }
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!(
                "停止 VPN 失败，退出码: {:?}\nstdout: {}\nstderr: {}",
                output.status.code(),
                stdout.trim(),
                stderr.trim()
            ))
        }
    }
}
