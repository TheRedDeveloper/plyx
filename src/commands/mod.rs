pub mod add;
pub mod apk;
pub mod completions;
pub mod easter_egg;
pub mod help;
pub mod init;
pub mod ios;
pub mod web;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Read the `[package] name` from Cargo.toml in the current directory.
pub(crate) fn read_crate_name() -> Result<String, String> {
    let cargo_str =
        fs::read_to_string("Cargo.toml").map_err(|e| format!("Failed to read Cargo.toml: {e}"))?;
    let doc: toml_edit::DocumentMut = cargo_str
        .parse()
        .map_err(|e| format!("Failed to parse Cargo.toml: {e}"))?;

    doc.get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No [package] name found in Cargo.toml.".to_string())
}

/// Get the target directory from `cargo metadata`. Handles workspaces correctly.
pub(crate) fn target_directory() -> Result<PathBuf, String> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|e| format!("Failed to run cargo metadata: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Failed to parse cargo metadata output: {e}"))?;

    json.get("target_directory")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .ok_or_else(|| "No target_directory in cargo metadata output.".to_string())
}
