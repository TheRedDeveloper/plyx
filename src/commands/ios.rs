use std::fs;
use std::path::Path;
use std::process::Command;

use crate::templates;
use crate::tui;

pub fn run(device: bool, actions: bool, auto: bool) {
    if let Err(e) = run_inner(device, actions, auto) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_inner(device: bool, actions: bool, auto: bool) -> Result<(), String> {
    if !Path::new("Cargo.toml").exists() {
        return Err(
            "No Cargo.toml found. Run this from the root of a ply-engine project.".to_string(),
        );
    }
    let crate_name = super::read_crate_name()?;

    // ── --actions: generate GitHub Actions workflow ──────────────────────
    if actions {
        return generate_actions_workflow(&crate_name);
    }

    // ── macOS-only check ────────────────────────────────────────────────
    if std::env::consts::OS != "macos" {
        return Err(
            "iOS builds require macOS with Xcode installed.\n\
             You can use `plyx ios --actions` to generate a GitHub Actions\n\
             workflow that builds on macOS CI runners."
                .to_string(),
        );
    }

    // ── Check Xcode / xcrun ─────────────────────────────────────────────
    let xcrun_ok = Command::new("xcrun")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !xcrun_ok {
        return Err(
            "Xcode command-line tools not found.\n\
             Install them with: xcode-select --install"
                .to_string(),
        );
    }

    if device {
        build_device(&crate_name, auto)
    } else {
        build_simulator(&crate_name, auto)
    }
}

fn build_simulator(crate_name: &str, _auto: bool) -> Result<(), String> {
    let target = simulator_target();

    // Ensure Rust target is installed
    ensure_rust_target(target)?;

    // 1. cargo build
    println!("Building for {target} (release)...");
    let status = Command::new("cargo")
        .args(["build", "--release", "--target", target])
        .status()
        .map_err(|e| format!("Failed to run cargo: {e}"))?;
    if !status.success() {
        return Err("cargo build failed.".to_string());
    }

    // 2. Create .app bundle
    let app_dir = format!("build/ios/{crate_name}.app");
    let app_path = Path::new(&app_dir);
    create_app_bundle(crate_name, target, app_path)?;

    // 3. Boot simulator if needed
    boot_simulator_if_needed()?;

    // 4. Install
    println!("Installing to simulator...");
    let status = Command::new("xcrun")
        .args(["simctl", "install", "booted", &app_dir])
        .status()
        .map_err(|e| format!("Failed to run xcrun simctl install: {e}"))?;
    if !status.success() {
        return Err("Failed to install app in simulator.".to_string());
    }

    // 5. Launch
    let bundle_id = format!("com.{}", crate_name.replace('-', "-"));
    println!("Launching {bundle_id} in simulator...");
    let status = Command::new("xcrun")
        .args(["simctl", "launch", "booted", &bundle_id])
        .status()
        .map_err(|e| format!("Failed to run xcrun simctl launch: {e}"))?;
    if !status.success() {
        return Err("Failed to launch app in simulator.".to_string());
    }

    println!("\nApp running in iOS Simulator.");
    println!(
        "Logs: xcrun simctl spawn booted log stream \
         --predicate 'processImagePath endswith \"{}\"'",
        crate_name
    );
    Ok(())
}

/// Determine the correct simulator target for this Mac's architecture.
fn simulator_target() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "aarch64-apple-ios-sim",
        _ => "x86_64-apple-ios",
    }
}

/// Boot an iOS Simulator if none is currently booted.
fn boot_simulator_if_needed() -> Result<(), String> {
    // Check if any simulator is already booted
    let output = Command::new("xcrun")
        .args(["simctl", "list", "devices", "available", "-j"])
        .output()
        .map_err(|e| format!("Failed to list simulators: {e}"))?;
    let json_str =
        String::from_utf8(output.stdout).map_err(|e| format!("Invalid simctl output: {e}"))?;

    // Simple check: is any device booted?
    if json_str.contains("\"state\" : \"Booted\"") {
        return Ok(());
    }

    // Find the first available iPhone
    let device_id = find_first_iphone(&json_str)?;
    println!("Booting simulator {device_id}...");
    let status = Command::new("xcrun")
        .args(["simctl", "boot", &device_id])
        .status()
        .map_err(|e| format!("Failed to boot simulator: {e}"))?;
    if !status.success() {
        return Err("Failed to boot iOS Simulator.".to_string());
    }

    // Open the Simulator app so the user can see it
    let _ = Command::new("open")
        .arg("/Applications/Xcode.app/Contents/Developer/Applications/Simulator.app/")
        .status();

    Ok(())
}

/// Parse `xcrun simctl list devices available -j` to find the first iPhone device UDID.
fn find_first_iphone(json_str: &str) -> Result<String, String> {
    let json: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("Failed to parse simctl JSON: {e}"))?;

    let devices = json
        .get("devices")
        .and_then(|d| d.as_object())
        .ok_or("Unexpected simctl JSON structure")?;

    // Look through runtimes for an iPhone
    for (_runtime, device_list) in devices {
        let list = match device_list.as_array() {
            Some(l) => l,
            None => continue,
        };
        for device in list {
            let name = device.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let udid = device.get("udid").and_then(|u| u.as_str()).unwrap_or("");
            let available = device
                .get("isAvailable")
                .and_then(|a| a.as_bool())
                .unwrap_or(false);
            if available && name.contains("iPhone") && !udid.is_empty() {
                return Ok(udid.to_string());
            }
        }
    }

    Err(
        "No available iPhone simulator found.\n\
         Open Xcode and install at least one iOS Simulator runtime."
            .to_string(),
    )
}

fn build_device(crate_name: &str, auto: bool) -> Result<(), String> {
    let target = "aarch64-apple-ios";

    // Ensure Rust target
    ensure_rust_target(target)?;

    // Check ios-deploy
    ensure_ios_deploy(auto)?;

    // 1. cargo build
    println!("Building for {target} (release)...");
    let status = Command::new("cargo")
        .args(["build", "--release", "--target", target])
        .status()
        .map_err(|e| format!("Failed to run cargo: {e}"))?;
    if !status.success() {
        return Err("cargo build failed.".to_string());
    }

    // 2. Create .app bundle
    let app_dir = format!("build/ios/{crate_name}.app");
    let app_path = Path::new(&app_dir);
    create_app_bundle(crate_name, target, app_path)?;

    // 3. Check provisioning profile
    let provision_path = app_path.join("embedded.mobileprovision");
    if !provision_path.exists() {
        return Err(format!(
            "No embedded.mobileprovision found in {app_dir}/.\n\n\
             To deploy to a real device, you need a provisioning profile.\n\
             Steps:\n\
             1. Open Xcode and sign in with your Apple ID\n\
             2. Create a dummy iOS project with bundle ID \"com.{crate_name}\"\n\
             3. Run it on your device (this fetches the provisioning profile)\n\
             4. Copy the .mobileprovision from ~/Library/MobileDevice/Provisioning Profiles/\n\
             5. Place it at: {app_dir}/embedded.mobileprovision\n\n\
             Then re-run `plyx ios --device`."
        ));
    }

    // 4. Check entitlements file
    let entitlements_path = format!("{crate_name}.entitlements.xml");
    if !Path::new(&entitlements_path).exists() {
        return Err(format!(
            "No {entitlements_path} found.\n\n\
             Create it with your team ID and bundle ID:\n\n\
             <?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ...>\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
               <key>application-identifier</key>\n\
               <string>YOUR_TEAM_ID.com.{crate_name}</string>\n\
             </dict>\n\
             </plist>\n\n\
             Find your team ID with:\n\
             security cms -D -i {app_dir}/embedded.mobileprovision | grep -A1 TeamIdentifier"
        ));
    }

    // 5. Sign
    let signing_identity = find_signing_identity()?;
    println!("Signing with identity: {signing_identity}...");

    let status = Command::new("codesign")
        .args([
            "--force",
            "--timestamp=none",
            "--sign",
            &signing_identity,
            "--entitlements",
            &entitlements_path,
            &app_dir,
        ])
        .status()
        .map_err(|e| format!("Failed to run codesign: {e}"))?;
    if !status.success() {
        return Err("Code signing failed.".to_string());
    }

    // 6. Deploy
    println!("Deploying to device...");
    let status = Command::new("ios-deploy")
        .args(["-b", &app_dir])
        .status()
        .map_err(|e| format!("Failed to run ios-deploy: {e}"))?;
    if !status.success() {
        return Err("ios-deploy failed. Make sure a device is connected.".to_string());
    }

    println!("\nApp deployed to device.");
    Ok(())
}

/// Find first code signing identity for iOS.
fn find_signing_identity() -> Result<String, String> {
    let output = Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
        .map_err(|e| format!("Failed to run security: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Each line looks like:  1) HEXID "Apple Development: name@email.com (TEAMID)"
    // Extract the hex ID from the first valid line.
    for line in stdout.lines() {
        let line = line.trim();
        if line.starts_with(|c: char| c.is_ascii_digit()) && line.contains(')') {
            // Extract the hex between ") " and " \""
            if let Some(hex_start) = line.find(") ") {
                let rest = &line[hex_start + 2..];
                if let Some(hex_end) = rest.find(' ') {
                    let hex_id = &rest[..hex_end];
                    if hex_id.len() == 40 && hex_id.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Ok(hex_id.to_string());
                    }
                }
            }
        }
    }

    Err(
        "No code signing identity found.\n\
         Make sure you have a development certificate in your Keychain.\n\
         Open Xcode → Preferences → Accounts to set one up."
            .to_string(),
    )
}

/// Check that ios-deploy is available, offer to install via brew if not.
fn ensure_ios_deploy(auto: bool) -> Result<(), String> {
    let has_it = Command::new("which")
        .arg("ios-deploy")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if has_it {
        return Ok(());
    }

    println!("ios-deploy is required to deploy to a real iOS device but was not found.");
    let should_install = if auto {
        true
    } else {
        tui::confirm("Install ios-deploy via Homebrew (brew install ios-deploy)?")
            .map_err(|e| format!("TUI error: {e}"))?
    };

    if !should_install {
        return Err(
            "ios-deploy is required for device deployment.\n\
             Install it manually: brew install ios-deploy"
                .to_string(),
        );
    }

    // Check that brew is available
    let has_brew = Command::new("which")
        .arg("brew")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !has_brew {
        return Err(
            "Homebrew is not installed. Install ios-deploy manually:\n\
             1. Install Homebrew: https://brew.sh\n\
             2. Run: brew install ios-deploy"
                .to_string(),
        );
    }

    println!("Installing ios-deploy...");
    let status = Command::new("brew")
        .args(["install", "ios-deploy"])
        .status()
        .map_err(|e| format!("Failed to run brew: {e}"))?;
    if !status.success() {
        return Err("Failed to install ios-deploy via Homebrew.".to_string());
    }
    println!("  ios-deploy installed.");
    Ok(())
}

/// Ensure the given Rust target is installed, adding it silently if needed.
fn ensure_rust_target(target: &str) -> Result<(), String> {
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map_err(|e| format!("Failed to run rustup: {e}"))?;
    let installed = String::from_utf8_lossy(&output.stdout);
    if installed.lines().any(|l| l.trim() == target) {
        return Ok(());
    }

    println!("Installing Rust target {target}...");
    let status = Command::new("rustup")
        .args(["target", "add", target])
        .status()
        .map_err(|e| format!("Failed to run rustup: {e}"))?;
    if !status.success() {
        return Err(format!("Failed to install Rust target {target}."));
    }
    Ok(())
}

/// Create the .app bundle directory with binary, Info.plist, and assets.
fn create_app_bundle(crate_name: &str, target: &str, app_path: &Path) -> Result<(), String> {
    fs::create_dir_all(app_path)
        .map_err(|e| format!("Failed to create {}: {e}", app_path.display()))?;

    // Copy binary
    let binary_src = Path::new("target")
        .join(target)
        .join("release")
        .join(crate_name);
    let binary_src = if binary_src.exists() {
        binary_src
    } else {
        // Try underscore variant
        let alt = Path::new("target")
            .join(target)
            .join("release")
            .join(crate_name.replace('-', "_"));
        if alt.exists() {
            alt
        } else {
            return Err(format!(
                "Expected binary at {} but it doesn't exist.",
                binary_src.display()
            ));
        }
    };
    let binary_name = binary_src
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    fs::copy(&binary_src, app_path.join(&binary_name))
        .map_err(|e| format!("Failed to copy binary: {e}"))?;
    println!("  Copied binary");

    // Generate Info.plist (don't overwrite)
    let plist_path = app_path.join("Info.plist");
    if !plist_path.exists() {
        let bundle_id = format!("com.{}", crate_name.replace('-', "-"));
        let display_name = crate_name
            .split('-')
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    Some(ch) => ch.to_uppercase().to_string() + c.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        let plist = templates::generate_info_plist(&binary_name, &bundle_id, &display_name);
        fs::write(&plist_path, plist)
            .map_err(|e| format!("Failed to write Info.plist: {e}"))?;
        println!("  Generated Info.plist");
    } else {
        println!("  Info.plist already exists, skipping");
    }

    // Copy assets/ if present
    let assets_src = Path::new("assets");
    let assets_dst = app_path.join("assets");
    if assets_src.exists() {
        copy_dir_recursive(assets_src, &assets_dst)?;
        println!("  Copied assets/");
    }

    Ok(())
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("Failed to create {}: {e}", dst.display()))?;

    let entries =
        fs::read_dir(src).map_err(|e| format!("Failed to read {}: {e}", src.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {e}"))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("Failed to copy {}: {e}", src_path.display()))?;
        }
    }
    Ok(())
}

fn generate_actions_workflow(crate_name: &str) -> Result<(), String> {
    let workflow_dir = Path::new(".github/workflows");
    let workflow_path = workflow_dir.join("ios.yml");

    if workflow_path.exists() {
        println!("{} already exists, skipping.", workflow_path.display());
        return Ok(());
    }

    fs::create_dir_all(workflow_dir)
        .map_err(|e| format!("Failed to create {}: {e}", workflow_dir.display()))?;

    let workflow = templates::generate_ios_actions_workflow(crate_name);
    fs::write(&workflow_path, workflow)
        .map_err(|e| format!("Failed to write {}: {e}", workflow_path.display()))?;

    println!("Generated {}", workflow_path.display());
    println!("Commit and push to trigger the workflow on GitHub.");
    Ok(())
}
