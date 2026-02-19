use crate::tui;
use std::fs;
use std::path::Path;
use std::process::Command;

const DOCKER_IMAGE: &str = "ghcr.io/thereddeveloper/plyx";

pub fn run(native: bool, install: bool, auto: bool) {
    let result = if native {
        run_native(install, auto)
    } else {
        run_docker(install, auto)
    };
    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

// ── Docker mode ─────────────────────────────────────────────────────────

fn run_docker(install: bool, auto: bool) -> Result<(), String> {
    if !Path::new("Cargo.toml").exists() {
        return Err(
            "No Cargo.toml found. Run this from the root of a ply-engine project.".to_string(),
        );
    }

    let crate_name = super::read_crate_name()?;

    // ── 1. Check Docker ────────────────────────────────────────────────
    check_docker(auto)?;

    // ── 2. Pull / ensure Docker image ──────────────────────────────────
    ensure_docker_image()?;

    // ── 3. If build.rs exists, run cargo check to trigger build scripts
    if Path::new("build.rs").exists() {
        println!("Running cargo check to trigger build scripts...");
        let status = Command::new("cargo")
            .args(["check"])
            .status()
            .map_err(|e| format!("Failed to run cargo check: {e}"))?;
        if !status.success() {
            return Err("cargo check failed.".to_string());
        }
    }

    // ── 4. Generate temporary overlay files ─────────────────────────────
    let tmp_dir = std::env::temp_dir().join("plyx-apk-overlay");
    fs::create_dir_all(&tmp_dir)
        .map_err(|e| format!("Failed to create temp dir: {e}"))?;

    let tmp_cargo = tmp_dir.join("Cargo.toml");
    let has_build_rs = Path::new("build.rs").exists();

    let project_dir = std::env::current_dir()
        .map_err(|e| format!("Failed to get current directory: {e}"))?;

    let path_dep_mounts = generate_overlay_cargo_toml(&tmp_cargo, &project_dir, true)?;

    // Only create a stub build.rs if the project has one — avoids Docker
    // bind mount creating an empty file on the host.
    let tmp_build_rs = tmp_dir.join("build.rs");
    if has_build_rs {
        fs::write(&tmp_build_rs, "fn main() {}\n")
            .map_err(|e| format!("Failed to write stub build.rs: {e}"))?;
    }

    // ── 5. Run Docker ──────────────────────────────────────────────────
    let project_dir_str = project_dir.to_str()
        .ok_or("Project path contains non-UTF-8 characters")?;

    let mut docker_args = vec![
        "run".to_string(), "--rm".to_string(),
        "-v".to_string(), format!("{project_dir_str}:/root/src"),
        "-v".to_string(), format!("{}:/root/src/Cargo.toml", tmp_cargo.display()),
    ];

    // Mount path dependencies
    for (host_path, container_path) in &path_dep_mounts {
        docker_args.push("-v".to_string());
        docker_args.push(format!("{host_path}:{container_path}"));
    }

    // Only overlay build.rs if the project has one
    if has_build_rs {
        docker_args.push("-v".to_string());
        docker_args.push(format!("{}:/root/src/build.rs", tmp_build_rs.display()));
    }

    docker_args.extend([
        "-e".to_string(), "CARGO_TARGET_DIR=target".to_string(),
        "-w".to_string(), "/root/src".to_string(),
        DOCKER_IMAGE.to_string(),
        "cargo".to_string(), "quad-apk".to_string(),
        "build".to_string(), "--release".to_string(),
    ]);

    println!("Building APK in Docker...");
    let status = Command::new("docker")
        .args(&docker_args)
        .status()
        .map_err(|e| format!("Failed to run Docker: {e}"))?;

    // ── 6. Clean up temp files ─────────────────────────────────────────
    let _ = fs::remove_dir_all(&tmp_dir);

    if !status.success() {
        return Err("Docker build failed.".to_string());
    }

    // ── 7. Locate APK ──────────────────────────────────────────────────
    let apk_path = format!(
        "target/android-artifacts/release/apk/{crate_name}.apk"
    );
    if Path::new(&apk_path).exists() {
        println!("\nAPK built: {apk_path}");
    } else {
        println!("\nBuild complete. Check target/android-artifacts/release/apk/ for the APK.");
    }

    // ── 8. Install via adb ─────────────────────────────────────────────
    if install {
        install_apk(&apk_path)?;
    }

    Ok(())
}

// ── Docker helpers ──────────────────────────────────────────────────────

fn check_docker(auto: bool) -> Result<(), String> {
    let has_docker = Command::new("docker")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if has_docker {
        return Ok(());
    }

    if auto {
        return Err("Docker is not installed. Install Docker and try again.".to_string());
    }

    Err(
        "Docker is not installed.\n\
         Install Docker: https://docs.docker.com/get-docker/\n\
         Then run `plyx apk` again."
            .to_string(),
    )
}

fn ensure_docker_image() -> Result<(), String> {
    // Check if image exists locally
    let check = Command::new("docker")
        .args(["image", "inspect", DOCKER_IMAGE])
        .output()
        .map_err(|e| format!("Failed to run docker: {e}"))?;

    if check.status.success() {
        return Ok(());
    }

    // Pull the image
    println!("Pulling Docker image {DOCKER_IMAGE}...");
    let status = Command::new("docker")
        .args(["pull", DOCKER_IMAGE])
        .status()
        .map_err(|e| format!("Failed to pull Docker image: {e}"))?;

    if !status.success() {
        return Err(format!(
            "Failed to pull Docker image {DOCKER_IMAGE}. \
             Make sure you have internet access and Docker is running."
        ));
    }

    Ok(())
}

// ── Cargo.toml overlay ──────────────────────────────────────────────────

/// Generate a modified Cargo.toml for Android building:
/// - Strip `[build-dependencies]`
/// - Rewrite `path` dependencies to Docker mount paths
/// - Add `[package.metadata.android]` if missing
///
/// Returns a list of (host_path, container_path) volume mounts needed for
/// path dependencies.
fn generate_overlay_cargo_toml(
    dest: &Path,
    project_dir: &Path,
    docker_mode: bool,
) -> Result<Vec<(String, String)>, String> {
    let cargo_str =
        fs::read_to_string("Cargo.toml").map_err(|e| format!("Failed to read Cargo.toml: {e}"))?;

    let mut doc: toml_edit::DocumentMut = cargo_str
        .parse()
        .map_err(|e| format!("Failed to parse Cargo.toml: {e}"))?;

    // Strip [build-dependencies]
    doc.remove("build-dependencies");

    // Rewrite path dependencies: resolve to absolute paths, and in Docker mode
    // use container mount paths instead.
    let mut mounts: Vec<(String, String)> = Vec::new();
    if let Some(deps) = doc.get_mut("dependencies").and_then(|d| d.as_table_like_mut()) {
        for (name, value) in deps.iter_mut() {
            if let Some(tbl) = value.as_inline_table_mut() {
                if let Some(rel_path) = tbl.get("path").and_then(|p| p.as_str()).map(String::from) {
                    let abs = project_dir.join(&rel_path);
                    let abs = abs.canonicalize().unwrap_or(abs);
                    let host_str = abs.to_string_lossy().to_string();
                    if docker_mode {
                        let container_path = format!("/root/deps/{name}");
                        tbl.insert("path", toml_edit::Value::from(container_path.as_str()));
                        mounts.push((host_str, container_path));
                    } else {
                        tbl.insert("path", toml_edit::Value::from(host_str.as_str()));
                    }
                }
            } else if let Some(tbl) = value.as_table_mut() {
                if let Some(rel_path) = tbl.get("path").and_then(|i| i.as_str()).map(String::from) {
                    let abs = project_dir.join(&rel_path);
                    let abs = abs.canonicalize().unwrap_or(abs);
                    let host_str = abs.to_string_lossy().to_string();
                    if docker_mode {
                        let container_path = format!("/root/deps/{name}");
                        tbl.insert("path", toml_edit::Item::Value(toml_edit::Value::from(container_path.as_str())));
                        mounts.push((host_str, container_path));
                    } else {
                        tbl.insert("path", toml_edit::Item::Value(toml_edit::Value::from(host_str.as_str())));
                    }
                }
            }
        }
    }

    // Add [package.metadata.android] if missing
    ensure_android_metadata(&mut doc);

    fs::write(dest, doc.to_string())
        .map_err(|e| format!("Failed to write overlay Cargo.toml: {e}"))?;

    Ok(mounts)
}

fn ensure_android_metadata(doc: &mut toml_edit::DocumentMut) {
    // Ensure [package.metadata.android] exists
    if doc.get("package").is_none() {
        return;
    }

    let package = &mut doc["package"];

    // Ensure metadata table
    if package.get("metadata").is_none() {
        package["metadata"] = toml_edit::Item::Table(toml_edit::Table::new());
    }

    let metadata = &mut package["metadata"];
    if metadata.get("android").is_none() {
        metadata["android"] = toml_edit::Item::Table(toml_edit::Table::new());
    }

    let android = &mut metadata["android"];

    // Set defaults if not present
    if android.get("assets").is_none() {
        android["assets"] = toml_edit::value("assets/");
    }
    if android.get("build_targets").is_none() {
        let mut arr = toml_edit::Array::new();
        arr.push("aarch64-linux-android");
        android["build_targets"] = toml_edit::value(arr);
    }

    // Ensure activity_attributes with exported = true
    let aa_key = "activity_attributes";
    if android.get(aa_key).is_none() {
        android[aa_key] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    if let Some(aa) = android[aa_key].as_table_mut() {
        if aa.get("android:exported").is_none() {
            aa.insert(
                "android:exported",
                toml_edit::Item::Value(toml_edit::Value::from("true")),
            );
        }
    }
}

// ── ADB install ─────────────────────────────────────────────────────────

fn install_apk(apk_path: &str) -> Result<(), String> {
    let adb = find_adb()?;

    println!("Installing APK via adb...");
    let status = Command::new(&adb)
        .args(["install", "-r", apk_path])
        .status()
        .map_err(|e| format!("Failed to run adb: {e}"))?;

    if !status.success() {
        return Err("adb install failed.".to_string());
    }

    println!("APK installed successfully.");
    Ok(())
}

fn find_adb() -> Result<String, String> {
    // Check PATH first
    if Command::new("adb")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return Ok("adb".to_string());
    }

    // Platform-specific common locations
    let candidates = [
        // Linux
        "/usr/bin/adb",
        "/usr/local/bin/adb",
        // macOS (Android Studio default)
        &format!(
            "{}/Library/Android/sdk/platform-tools/adb",
            std::env::var("HOME").unwrap_or_default()
        ),
        // Linux (Android Studio default)
        &format!(
            "{}/Android/Sdk/platform-tools/adb",
            std::env::var("HOME").unwrap_or_default()
        ),
    ];

    for candidate in &candidates {
        if Path::new(candidate).exists() {
            return Ok(candidate.to_string());
        }
    }

    // Check ANDROID_HOME
    if let Ok(android_home) = std::env::var("ANDROID_HOME") {
        let adb = format!("{android_home}/platform-tools/adb");
        if Path::new(&adb).exists() {
            return Ok(adb);
        }
    }

    Err(
        "adb not found. Install Android SDK platform-tools or add adb to PATH.".to_string(),
    )
}

// ── Native mode ─────────────────────────────────────────────────────────

fn run_native(install: bool, auto: bool) -> Result<(), String> {
    if !Path::new("Cargo.toml").exists() {
        return Err(
            "No Cargo.toml found. Run this from the root of a ply-engine project.".to_string(),
        );
    }

    let crate_name = super::read_crate_name()?;

    // ── 1. Check NDK_HOME ──────────────────────────────────────────────
    check_ndk(auto)?;

    // ── 2. Check ANDROID_HOME ──────────────────────────────────────────
    check_android_home(auto)?;

    // ── 3. Check cargo-quad-apk ────────────────────────────────────────
    check_cargo_quad_apk()?;

    // ── 4. Symlink-based project overlay ───────────────────────────────
    let project_dir = std::env::current_dir()
        .map_err(|e| format!("Failed to get current directory: {e}"))?;

    let tmp_dir = std::env::temp_dir().join("plyx-apk-native");
    // Clean any previous overlay
    let _ = fs::remove_dir_all(&tmp_dir);
    fs::create_dir_all(&tmp_dir)
        .map_err(|e| format!("Failed to create temp dir: {e}"))?;

    // Symlink everything except Cargo.toml and build.rs
    create_symlink_overlay(&project_dir, &tmp_dir)?;

    // Write modified Cargo.toml (path dep mounts are unused in native mode)
    generate_overlay_cargo_toml(&tmp_dir.join("Cargo.toml"), &project_dir, false)?;

    // Write stub build.rs
    fs::write(tmp_dir.join("build.rs"), "fn main() {}\n")
        .map_err(|e| format!("Failed to write stub build.rs: {e}"))?;

    // ── 5. Build ───────────────────────────────────────────────────────
    println!("Building APK with native NDK...");
    let status = Command::new("cargo")
        .args(["quad-apk", "build", "--release"])
        .current_dir(&tmp_dir)
        .status()
        .map_err(|e| format!("Failed to run cargo quad-apk: {e}"))?;

    if !status.success() {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err("Native APK build failed.".to_string());
    }

    // Copy APK back to project
    let apk_src = tmp_dir
        .join("target")
        .join("android-artifacts")
        .join("release")
        .join("apk")
        .join(format!("{crate_name}.apk"));
    let apk_dst_dir = project_dir
        .join("target")
        .join("android-artifacts")
        .join("release")
        .join("apk");
    fs::create_dir_all(&apk_dst_dir)
        .map_err(|e| format!("Failed to create APK output dir: {e}"))?;
    let apk_dst = apk_dst_dir.join(format!("{crate_name}.apk"));

    if apk_src.exists() {
        fs::copy(&apk_src, &apk_dst)
            .map_err(|e| format!("Failed to copy APK: {e}"))?;
        println!("\nAPK built: {}", apk_dst.display());
    } else {
        println!("\nBuild complete. Check the overlay dir for APK output.");
    }

    // ── 6. Clean up overlay ────────────────────────────────────────────
    let _ = fs::remove_dir_all(&tmp_dir);

    // ── 7. Install via adb ─────────────────────────────────────────────
    if install {
        install_apk(&apk_dst.to_string_lossy())?;
    }

    Ok(())
}

/// Create a symlink overlay: symlink all entries in `src` into `dst`,
/// except Cargo.toml and build.rs which will be written separately.
fn create_symlink_overlay(src: &Path, dst: &Path) -> Result<(), String> {
    let entries = fs::read_dir(src)
        .map_err(|e| format!("Failed to read project dir: {e}"))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {e}"))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip files we'll replace
        if name_str == "Cargo.toml" || name_str == "build.rs" {
            continue;
        }

        let link_path = dst.join(&name);
        let target = entry.path();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, &link_path)
                .map_err(|e| format!("Failed to symlink {}: {e}", name_str))?;
        }

        #[cfg(not(unix))]
        {
            // Fallback: copy for non-Unix platforms
            if target.is_dir() {
                super::super::commands::web::copy_dir_recursive(&target, &link_path)?;
            } else {
                fs::copy(&target, &link_path)
                    .map_err(|e| format!("Failed to copy {}: {e}", name_str))?;
            }
        }
    }

    Ok(())
}

// ── NDK / SDK checks ───────────────────────────────────────────────────

fn check_ndk(auto: bool) -> Result<(), String> {
    // If NDK_HOME is set, validate it
    if let Ok(ndk_home) = std::env::var("NDK_HOME") {
        return validate_ndk(&ndk_home);
    }

    // Check common default locations
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let common_paths = [
        format!("{home}/android-ndk-r25"),
        format!("{home}/Android/Sdk/ndk/25.2.9519653"),
        format!("{home}/Android/Sdk/ndk/25.1.8937393"),
        "/usr/local/android-ndk-r25".to_string(),
        "/opt/android-ndk-r25".to_string(),
    ];

    for path in &common_paths {
        if Path::new(path).exists() {
            if let Ok(()) = validate_ndk(path) {
                std::env::set_var("NDK_HOME", path);
                return Ok(());
            }
        }
    }

    // Also check ANDROID_HOME/ndk/ for any r25 variant
    if let Ok(android_home) = std::env::var("ANDROID_HOME") {
        let ndk_dir = Path::new(&android_home).join("ndk");
        if ndk_dir.exists() {
            if let Ok(entries) = fs::read_dir(&ndk_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with("25.") {
                            let path_str = p.to_string_lossy().to_string();
                            if let Ok(()) = validate_ndk(&path_str) {
                                std::env::set_var("NDK_HOME", &path_str);
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }
    }

    // Not found anywhere
    if auto {
        return Err(
            "NDK_HOME is not set and NDK r25 was not found in common locations.".to_string(),
        );
    }

    // Interactive: offer to download
    let default_path = format!("{home}/android-ndk-r25");
    println!("Android NDK r25 not found. plyx requires NDK r25 for native builds.");
    let yes = tui::confirm(&format!(
        "Download and install NDK r25 to {default_path}?"
    ))?;

    if !yes {
        return Err(
            "NDK r25 is required for native builds. Set NDK_HOME and try again.".to_string(),
        );
    }

    download_ndk(&default_path)?;
    std::env::set_var("NDK_HOME", &default_path);
    println!("  NDK r25 installed to {default_path}");
    println!("  Tip: Add `export NDK_HOME={default_path}` to your shell profile.");
    Ok(())
}

/// Validate that a path contains NDK r25.
fn validate_ndk(path: &str) -> Result<(), String> {
    let ndk_path = Path::new(path);
    if !ndk_path.exists() {
        return Err(format!("NDK path {path} doesn't exist."));
    }
    let source_props = ndk_path.join("source.properties");
    if source_props.exists() {
        let content = fs::read_to_string(&source_props).unwrap_or_default();
        if !content.contains("25.") {
            return Err(format!(
                "NDK at {path} is not r25. plyx requires NDK r25 specifically."
            ));
        }
    }
    println!("  NDK r25 found at {path}");
    Ok(())
}

/// Download and extract Android NDK r25.
fn download_ndk(dest: &str) -> Result<(), String> {
    let url = "https://dl.google.com/android/repository/android-ndk-r25-linux.zip";
    let tmp_zip = std::env::temp_dir().join("android-ndk-r25-linux.zip");

    println!("Downloading NDK r25 (this may take a while)...");

    let status = Command::new("wget")
        .args(["-q", "--show-progress", "-O"])
        .arg(&tmp_zip)
        .arg(url)
        .status()
        .or_else(|_| {
            // Fallback to curl if wget not available
            Command::new("curl")
                .args(["-L", "-o"])
                .arg(&tmp_zip)
                .arg(url)
                .status()
        })
        .map_err(|e| format!("Failed to download NDK: {e}. Install wget or curl."))?;

    if !status.success() {
        return Err("NDK download failed.".to_string());
    }

    println!("Extracting NDK...");
    let parent = Path::new(dest)
        .parent()
        .ok_or("Invalid destination path")?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create destination: {e}"))?;

    let status = Command::new("unzip")
        .args(["-q", "-o"])
        .arg(&tmp_zip)
        .arg("-d")
        .arg(parent)
        .status()
        .map_err(|e| format!("Failed to extract NDK: {e}. Install unzip."))?;

    let _ = fs::remove_file(&tmp_zip);

    if !status.success() {
        return Err("NDK extraction failed.".to_string());
    }

    Ok(())
}

fn check_android_home(auto: bool) -> Result<(), String> {
    // If ANDROID_HOME is set, validate it
    if let Ok(android_home) = std::env::var("ANDROID_HOME") {
        return validate_android_home(&android_home);
    }

    // Check common default locations
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let common_paths = [
        format!("{home}/Android/Sdk"),
        format!("{home}/android-sdk"),
        "/opt/android-sdk".to_string(),
        "/opt/android-sdk-linux".to_string(),
        "/usr/local/android-sdk".to_string(),
    ];

    for path in &common_paths {
        if Path::new(path).exists() {
            if let Ok(()) = validate_android_home(path) {
                std::env::set_var("ANDROID_HOME", path);
                return Ok(());
            }
        }
    }

    // Not found
    if auto {
        return Err(
            "ANDROID_HOME is not set and Android SDK was not found in common locations."
                .to_string(),
        );
    }

    // Interactive: offer to download
    let default_path = format!("{home}/android-sdk");
    println!("Android SDK not found. plyx requires the Android SDK for native builds.");
    let yes = tui::confirm(&format!(
        "Download and install Android SDK to {default_path}?"
    ))?;

    if !yes {
        return Err(
            "Android SDK is required for native builds. Set ANDROID_HOME and try again."
                .to_string(),
        );
    }

    download_sdk(&default_path)?;
    std::env::set_var("ANDROID_HOME", &default_path);
    println!("  Android SDK installed to {default_path}");
    println!("  Tip: Add `export ANDROID_HOME={default_path}` to your shell profile.");
    Ok(())
}

/// Validate that a path contains a usable Android SDK.
fn validate_android_home(path: &str) -> Result<(), String> {
    let home_path = Path::new(path);
    if !home_path.exists() {
        return Err(format!("Android SDK path {path} doesn't exist."));
    }

    let mut missing = Vec::new();
    if !home_path.join("platform-tools").exists() {
        missing.push("platform-tools");
    }
    if !home_path.join("platforms/android-36").exists()
        && !home_path.join("platforms").exists()
    {
        missing.push("platforms;android-36");
    }
    if !home_path.join("build-tools/36.0.0-rc5").exists()
        && !home_path.join("build-tools").exists()
    {
        missing.push("build-tools;36.0.0-rc5");
    }

    if !missing.is_empty() {
        return Err(format!(
            "Android SDK at {path} is missing components: {}",
            missing.join(", ")
        ));
    }

    println!("  Android SDK found at {path}");
    Ok(())
}

/// Download and set up the Android SDK with required components.
fn download_sdk(dest: &str) -> Result<(), String> {
    let cmdline_tools_url =
        "https://dl.google.com/android/repository/commandlinetools-linux-13114758_latest.zip";
    let tmp_zip = std::env::temp_dir().join("android-cmdline-tools.zip");

    fs::create_dir_all(dest)
        .map_err(|e| format!("Failed to create {dest}: {e}"))?;

    println!("Downloading Android SDK command-line tools...");
    let status = Command::new("wget")
        .args(["-q", "--show-progress", "-O"])
        .arg(&tmp_zip)
        .arg(cmdline_tools_url)
        .status()
        .or_else(|_| {
            Command::new("curl")
                .args(["-L", "-o"])
                .arg(&tmp_zip)
                .arg(cmdline_tools_url)
                .status()
        })
        .map_err(|e| format!("Failed to download SDK tools: {e}"))?;

    if !status.success() {
        return Err("SDK tools download failed.".to_string());
    }

    // Extract and arrange command-line tools
    let status = Command::new("unzip")
        .args(["-q", "-o"])
        .arg(&tmp_zip)
        .arg("-d")
        .arg(dest)
        .status()
        .map_err(|e| format!("Failed to extract SDK tools: {e}"))?;

    let _ = fs::remove_file(&tmp_zip);

    if !status.success() {
        return Err("SDK tools extraction failed.".to_string());
    }

    // Rearrange: cmdline-tools → cmdline-tools/latest
    let extracted = Path::new(dest).join("cmdline-tools");
    let latest_dir = Path::new(dest).join("cmdline-tools-tmp");
    if extracted.exists() {
        fs::rename(&extracted, &latest_dir)
            .map_err(|e| format!("Failed to rearrange SDK tools: {e}"))?;
        fs::create_dir_all(&extracted)
            .map_err(|e| format!("Failed to create cmdline-tools dir: {e}"))?;
        fs::rename(&latest_dir, extracted.join("latest"))
            .map_err(|e| format!("Failed to move SDK tools to latest: {e}"))?;
    }

    let sdkmanager = Path::new(dest)
        .join("cmdline-tools/latest/bin/sdkmanager")
        .display()
        .to_string();

    // Accept licenses
    println!("Accepting licenses...");
    let _ = Command::new("sh")
        .args(["-c", &format!("yes | {sdkmanager} --licenses > /dev/null 2>&1")])
        .status();

    // Install components
    for component in &[
        "platform-tools",
        "platforms;android-36",
        "build-tools;36.0.0-rc5",
    ] {
        println!("Installing {component}...");
        let status = Command::new("sh")
            .args(["-c", &format!("yes | {sdkmanager} \"{component}\"")])
            .status()
            .map_err(|e| format!("Failed to install {component}: {e}"))?;

        if !status.success() {
            eprintln!("Warning: Failed to install {component}");
        }
    }

    Ok(())
}

fn check_cargo_quad_apk() -> Result<(), String> {
    let has_it = Command::new("cargo")
        .args(["quad-apk", "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if has_it {
        return Ok(());
    }

    println!("Installing cargo-quad-apk...");
    let status = Command::new("cargo")
        .args([
            "install",
            "--git",
            "https://github.com/not-fl3/cargo-quad-apk",
        ])
        .status()
        .map_err(|e| format!("Failed to install cargo-quad-apk: {e}"))?;

    if !status.success() {
        return Err("Failed to install cargo-quad-apk.".to_string());
    }

    Ok(())
}
