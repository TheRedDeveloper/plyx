pub mod add;
pub mod apk;
pub mod completions;
pub mod easter_egg;
pub mod help;
pub mod init;
pub mod ios;
pub mod web;

use std::fs;

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
