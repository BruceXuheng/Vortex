use std::env;
use std::fs;
use std::path::Path;

/// Cargo 构建脚本：编译时查找并拷贝 APK。
///
/// 将找到的 APK 拷贝到 `OUT_DIR/vortex_app.apk`，供 `src/apk.rs`
/// 通过 `include_bytes!` 在编译时嵌入二进制。
///
/// 通过 `cargo:rustc-env=VORTEX_APK_PATH` 将路径传递给编译期，
/// 避免路径不一致问题。
///
/// 查找顺序：
/// 1. 环境变量 `VORTEX_APK`
/// 2. `../output/`（统一产物目录，package.sh 已拷贝）
/// 3. `../vortex_app/app/build/outputs/apk/release/`（Gradle 默认输出）
/// 4. `../vortex_app/app/build/outputs/apk/debug/`（调试构建）
///
/// 在 Gradle 默认输出目录中搜索任意 `.apk` 文件，
/// 不依赖固定文件名（Gradle 自定义了日期后缀）。
///
/// 如果都找不到，创建空占位文件——编译可以通过，
/// 但运行时 `apk::extract()` 会报错提示先构建 Android 项目。
fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir).join("vortex_app.apk");

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
            fs::copy(src_path, &out_path).expect("无法复制 APK 到 OUT_DIR");
            println!(
                "cargo:warning=APK 已嵌入: {} ({} 字节)",
                src,
                fs::metadata(&out_path).unwrap().len()
            );
        }
        println!("cargo:rerun-if-changed={}", src);
    } else {
        // 确保占位文件存在（空文件）
        if !out_path.exists() || fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0) > 0 {
            fs::write(&out_path, []).expect("无法创建 APK 占位文件");
            println!("cargo:warning=未找到 APK，将嵌入空占位——运行 install 需先构建 Android 项目");
        }
    }

    // 将 APK 路径传递给编译期，供 include_bytes! 使用
    println!("cargo:rustc-env=VORTEX_APK_PATH={}", out_path.display());
}

/// 查找 APK 文件。
///
/// 按优先级搜索：环境变量 → output/ → Gradle 默认输出目录。
/// 在目录中搜索任意 `.apk` 文件，不依赖固定文件名。
fn find_apk() -> Option<String> {
    // 1. 环境变量
    if let Ok(path) = env::var("VORTEX_APK") {
        if Path::new(&path).exists() {
            return Some(path);
        }
    }

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let base = Path::new(&manifest_dir);

    // 2. output/ 目录（package.sh 已拷贝）
    if let Some(path) = find_apk_in_dir(&base.join("../output")) {
        return Some(path);
    }

    // 3. Gradle 默认输出目录
    let candidates = [
        "../vortex_app/app/build/outputs/apk/release",
        "../vortex_app/app/build/outputs/apk/debug",
    ];

    for candidate in &candidates {
        let dir = base.join(candidate);
        if let Some(path) = find_apk_in_dir(&dir) {
            return Some(path);
        }
    }

    None
}

/// 在指定目录下查找第一个 `.apk` 文件。
fn find_apk_in_dir(dir: &Path) -> Option<String> {
    if !dir.is_dir() {
        return None;
    }
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "apk") {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    None
}
