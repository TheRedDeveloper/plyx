use std::fs;
use std::path::Path;
use std::process::Command;

use crate::templates;

const PLY_BUNDLE_URL: &str =
    "https://raw.githubusercontent.com/TheRedDeveloper/ply-engine/refs/heads/main/js/ply_bundle.js";

pub fn run(auto: bool) {
    if let Err(e) = run_inner(auto) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_inner(_auto: bool) -> Result<(), String> {
    // Must be in a project root with Cargo.toml
    if !Path::new("Cargo.toml").exists() {
        return Err(
            "No Cargo.toml found. Run this from the root of a ply-engine project.".to_string(),
        );
    }

    let crate_name = super::read_crate_name()?;
    // ── 1. cargo build ──────────────────────────────────────────────────
    println!("Building for wasm32-unknown-unknown (release)...");
    let status = Command::new("cargo")
        .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
        .status()
        .map_err(|e| format!("Failed to run cargo: {e}"))?;

    if !status.success() {
        return Err("cargo build failed.".to_string());
    }

    // ── 2. Create build/web/ ────────────────────────────────────────────
    let out = Path::new("build/web");
    fs::create_dir_all(out).map_err(|e| format!("Failed to create build/web/: {e}"))?;

    // ── 3. Copy assets/ → build/web/assets/ ─────────────────────────────
    let assets_src = Path::new("assets");
    let assets_dst = out.join("assets");
    if assets_src.exists() {
        copy_dir_recursive(assets_src, &assets_dst)?;
        println!("  Copied assets/");
    }

    // ── 4. Copy .wasm → build/web/app.wasm ──────────────────────────────
    // Try the crate name as-is first (Cargo preserves hyphens for bin targets),
    // then fall back to the underscore variant (lib/cdylib targets).
    let wasm_dir = Path::new("target/wasm32-unknown-unknown/release");
    let wasm_src = wasm_dir.join(format!("{crate_name}.wasm"));
    let wasm_src = if wasm_src.exists() {
        wasm_src
    } else {
        let alt = wasm_dir.join(format!("{}.wasm", crate_name.replace('-', "_")));
        if alt.exists() {
            alt
        } else {
            return Err(format!(
                "Expected wasm at {} (or with underscores) but neither exists.",
                wasm_dir.join(format!("{crate_name}.wasm")).display()
            ));
        }
    };

    if !wasm_src.exists() {
        return Err(format!(
            "Expected wasm at {} but it doesn't exist.",
            wasm_src.display()
        ));
    }
    fs::copy(&wasm_src, out.join("app.wasm"))
        .map_err(|e| format!("Failed to copy wasm: {e}"))?;
    println!("  Copied app.wasm");

    // ── 5. Generate index.html if it doesn't exist ──────────────────────
    if !Path::new("index.html").exists() {
        let title = crate_name
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
        let html = templates::INDEX_HTML.replace("{{TITLE}}", &title);
        fs::write("index.html", &html)
            .map_err(|e| format!("Failed to write index.html: {e}"))?;
        println!("  Generated index.html");
    }

    // ── 6. Copy index.html → build/web/index.html ──────────────────────
    fs::copy("index.html", out.join("index.html"))
        .map_err(|e| format!("Failed to copy index.html: {e}"))?;
    println!("  Copied index.html");

    // ── 7. Download ply_bundle.js (cached) ──────────────────────────────
    let bundle_dst = out.join("ply_bundle.js");
    download_bundle(&bundle_dst)?;

    // ── Done ────────────────────────────────────────────────────────────
    println!("\nWeb build ready at: build/web/");
    Ok(())
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst)
        .map_err(|e| format!("Failed to create {}: {e}", dst.display()))?;

    let entries = fs::read_dir(src)
        .map_err(|e| format!("Failed to read {}: {e}", src.display()))?;

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

/// Download ply_bundle.js from GitHub, using a local cache.
///
/// The cache lives at `~/.cache/plyx/ply_bundle.js`. We always try to
/// download the latest version; if the network request fails we fall back
/// to the cached copy.
fn download_bundle(dest: &Path) -> Result<(), String> {
    let cache_dir = {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        std::path::PathBuf::from(home).join(".cache").join("plyx")
    };
    fs::create_dir_all(&cache_dir).ok();
    let cache_path = cache_dir.join("ply_bundle.js");

    match fetch_bundle() {
        Ok(bytes) => {
            // Update cache
            fs::write(&cache_path, &bytes).ok();
            fs::write(dest, &bytes)
                .map_err(|e| format!("Failed to write ply_bundle.js: {e}"))?;
            println!("  Downloaded ply_bundle.js");
        }
        Err(fetch_err) => {
            if cache_path.exists() {
                eprintln!("Warning: Could not download ply_bundle.js, using cached version.");
                fs::copy(&cache_path, dest)
                    .map_err(|e| format!("Failed to copy cached bundle: {e}"))?;
                println!("  Copied ply_bundle.js (cached)");
            } else {
                return Err(format!("Failed to download ply_bundle.js: {fetch_err}"));
            }
        }
    }
    Ok(())
}

fn fetch_bundle() -> Result<Vec<u8>, String> {
    let response = ureq::get(PLY_BUNDLE_URL)
        .call()
        .map_err(|e| format!("{e}"))?;

    response
        .into_body()
        .with_config()
        .limit(10 * 1024 * 1024) // 10MB limit
        .read_to_vec()
        .map_err(|e| format!("{e}"))
}
