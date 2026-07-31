use std::fs;
use std::io;
use std::path::PathBuf;

/// 编译时嵌入的 APK 数据。
///
/// `build.rs` 在编译前将 APK 拷贝到 `target/vortex_app.apk`，
/// 此处通过 `include_bytes!` 在编译时将其嵌入二进制。
///
/// 如果编译时未找到 APK，嵌入空字节切片，
/// `extract()` 会返回错误提示先构建 Android 项目。
const APK_DATA: &[u8] = include_bytes!("../target/vortex_app.apk");

/// 将嵌入的 APK 写入临时文件，返回路径。
///
/// 写入 `std::env::temp_dir()/vortex/vortex_app.apk`。
/// 每次调用都会覆写，确保与嵌入版本一致。
pub fn extract() -> io::Result<PathBuf> {
    if APK_DATA.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "APK 未嵌入：编译时未找到 Android 构建产物。\n\
             请先执行: cd vortex_app && ./gradlew assembleRelease\n\
             然后重新编译: cd server && cargo build --release",
        ));
    }

    let dir = std::env::temp_dir().join("vortex");
    fs::create_dir_all(&dir)?;
    let path = dir.join("vortex_app.apk");
    fs::write(&path, APK_DATA)?;
    log::debug!("APK 已提取到: {}", path.display());
    Ok(path)
}
