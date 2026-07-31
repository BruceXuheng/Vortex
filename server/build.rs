use std::env;
use std::fs;
use std::path::Path;

/// Cargo 构建脚本：编译时查找并拷贝 APK。
///
/// 将找到的 APK 拷贝到 `target/vortex_app.apk`，供 `src/apk.rs`
/// 通过 `include_bytes!` 在编译时嵌入二进制。
///
/// 查找顺序：
/// 1. 环境变量 `VORTEX_APK`
/// 2. `../vortex_app/app/release/app-release.apk`
/// 3. `../vortex_app/app/build/outputs/apk/release/app-release.apk`
///
/// 如果都找不到，保留空的占位文件——编译可以通过，
/// 但运行时 `apk::extract()` 会报错提示先构建 Android 项目。
fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    // OUT_DIR 类似 target/debug/build/vortex-xxx/out
    // 需要放到 server/target/ 下，用更稳定的路径
    let target_dir = find_target_dir(&out_dir);
    let out_path = target_dir.join("vortex_app.apk");

    if let Some(src) = find_apk() {
        let src_path = Path::new(&src);
        // 只在文件变化时才拷贝（避免每次重编译）
        let need_copy = !out_path.exists()
            || fs::metadata(&src_path)
                .map(|m| m.len())
                .unwrap_or(0)
                != fs::metadata(&out_path)
                    .map(|m| m.len())
                    .unwrap_or(0);

        if need_copy {
            fs::copy(src_path, &out_path).expect("无法复制 APK 到 target/");
            println!("cargo:warning=APK 已嵌入: {} ({} 字节)", src, fs::metadata(&out_path).unwrap().len());
        }
        println!("cargo:rerun-if-changed={}", src);
    } else {
        // 确保占位文件存在（空文件）
        if !out_path.exists() || fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0) > 0 {
            fs::write(&out_path, []).expect("无法创建 APK 占位文件");
            println!("cargo:warning=未找到 APK，将嵌入空占位——运行 install 需先构建 Android 项目");
        }
    }
}

/// 从 OUT_DIR 反推 target 目录。
///
/// OUT_DIR 形如 `/path/to/server/target/debug/build/vortex-abc123/out`
/// 我们需要 `/path/to/server/target/`
fn find_target_dir(out_dir: &str) -> std::path::PathBuf {
    let path = Path::new(out_dir);
    // 往上找 target/ 目录
    let mut current = path;
    while let Some(parent) = current.parent() {
        if parent.file_name() == Some(std::ffi::OsStr::new("target")) {
            return parent.to_path_buf();
        }
        current = parent;
    }
    // 兜底：OUT_DIR 的 ../..
    path.parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf()
}

/// 查找 APK 文件。
fn find_apk() -> Option<String> {
    // 1. 环境变量
    if let Ok(path) = env::var("VORTEX_APK") {
        if Path::new(&path).exists() {
            return Some(path);
        }
    }

    // 2. 相对路径
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let base = Path::new(&manifest_dir);

    let candidates = [
        "../vortex_app/app/release/app-release.apk",
        "../vortex_app/app/build/outputs/apk/release/app-release.apk",
    ];

    for candidate in &candidates {
        let path = base.join(candidate);
        if path.exists() {
            return Some(path.to_string_lossy().into_owned());
        }
    }

    None
}
