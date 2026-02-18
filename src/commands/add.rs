use crate::fonts;
use crate::templates::{self, FEATURES};
use crate::tui;
use std::fs;
use std::path::Path;

pub fn run(args: Vec<String>) {
    if let Err(e) = run_inner(args) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_inner(args: Vec<String>) -> Result<(), String> {
    // Ensure we're in a ply-engine project (Cargo.toml exists).
    if !Path::new("Cargo.toml").exists() {
        return Err(
            "No Cargo.toml found. Run this from the root of a ply-engine project.".to_string(),
        );
    }

    if args.is_empty() {
        return interactive_add();
    }

    // Non-interactive: `plyx add <feature>` or `plyx add font <name...>`
    let first = args[0].to_lowercase();
    if first == "font" {
        if args.len() < 2 {
            return Err("Usage: plyx add font <name>".to_string());
        }
        let query = args[1..].join(" ");
        return add_font_by_name(&query);
    }

    // Treat as a feature key
    add_feature_by_key(&first)
}

// ── Interactive mode ────────────────────────────────────────────────────

fn interactive_add() -> Result<(), String> {
    let cargo_str =
        fs::read_to_string("Cargo.toml").map_err(|e| format!("Failed to read Cargo.toml: {e}"))?;

    let locked = detect_enabled_features(&cargo_str);
    let locked_refs: Vec<&str> = locked.iter().map(|s| s.as_str()).collect();

    let installed_fonts = detect_installed_fonts();

    let font_list = fonts::load_font_list()?;

    let result = tui::add_widget(
        "Add to project:",
        FEATURES,
        &font_list,
        &locked_refs,
        &installed_fonts,
        "",
    )?;

    if result.features.is_empty() && result.fonts.is_empty() {
        println!("Nothing to add.");
        return Ok(());
    }

    // Apply features
    if !result.features.is_empty() {
        apply_features(&result.features)?;
    }

    // Download fonts
    for font_name in &result.fonts {
        fonts::download(font_name, Path::new("assets/fonts"))?;
    }

    println!("\nDone!");
    Ok(())
}

// ── Non-interactive feature add ─────────────────────────────────────────

fn add_feature_by_key(key: &str) -> Result<(), String> {
    // Validate the key
    if !FEATURES.iter().any(|(k, _, _)| k == &key) {
        let valid: Vec<&str> = FEATURES.iter().map(|(k, _, _)| *k).collect();
        return Err(format!(
            "Unknown feature '{key}'. Valid features: {}",
            valid.join(", ")
        ));
    }

    let cargo_str =
        fs::read_to_string("Cargo.toml").map_err(|e| format!("Failed to read Cargo.toml: {e}"))?;

    let enabled = detect_enabled_features(&cargo_str);
    if enabled.iter().any(|e| e == key) {
        println!("Feature '{key}' is already enabled.");
        return Ok(());
    }

    apply_features(&[key.to_string()])?;
    println!("Added feature '{key}'.");
    Ok(())
}

// ── Non-interactive font add ────────────────────────────────────────────

fn add_font_by_name(query: &str) -> Result<(), String> {
    let font_list = fonts::load_font_list()?;
    let results = fonts::search(&font_list, query);

    if results.is_empty() {
        return Err(format!("No font found matching '{query}'."));
    }

    let best = results[0];

    // Check if already installed
    let installed = detect_installed_fonts();
    if installed.iter().any(|f| f.eq_ignore_ascii_case(best)) {
        println!("Font '{best}' is already installed.");
        return Ok(());
    }

    fonts::download(best, Path::new("assets/fonts"))?;
    println!("Added font '{best}'.");
    Ok(())
}

// ── Cargo.toml manipulation ─────────────────────────────────────────────

/// Detect which ply-engine features are currently enabled in Cargo.toml.
fn detect_enabled_features(cargo_str: &str) -> Vec<String> {
    let doc = match cargo_str.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    // Look for ply-engine in [dependencies]
    let deps = match doc.get("dependencies") {
        Some(d) => d,
        None => return Vec::new(),
    };

    let ply = match deps.get("ply-engine") {
        Some(p) => p,
        None => return Vec::new(),
    };

    let features_array = match ply.get("features") {
        Some(f) => f,
        None => return Vec::new(),
    };

    match features_array.as_array() {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        None => Vec::new(),
    }
}

/// Apply new features to Cargo.toml using toml_edit.
fn apply_features(new_features: &[String]) -> Result<(), String> {
    let cargo_str =
        fs::read_to_string("Cargo.toml").map_err(|e| format!("Failed to read Cargo.toml: {e}"))?;

    let mut doc: toml_edit::DocumentMut = cargo_str
        .parse()
        .map_err(|e| format!("Failed to parse Cargo.toml: {e}"))?;

    // Ensure [dependencies] exists
    if doc.get("dependencies").is_none() {
        doc["dependencies"] = toml_edit::Item::Table(toml_edit::Table::new());
    }

    let ply_dep = &mut doc["dependencies"]["ply-engine"];

    // If ply-engine is a simple string (no table), convert to inline table
    if ply_dep.is_str() {
        let git_url = ply_dep.as_str().unwrap_or("").to_string();
        let mut tbl = toml_edit::InlineTable::new();
        tbl.insert("git", toml_edit::Value::from(git_url));
        *ply_dep = toml_edit::Item::Value(toml_edit::Value::InlineTable(tbl));
    }

    // Get or create the features array
    if let Some(tbl) = ply_dep.as_inline_table_mut() {
        let features_val = tbl.get_or_insert("features", toml_edit::Value::Array(toml_edit::Array::new()));
        if let Some(arr) = features_val.as_array_mut() {
            let existing: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            for feat in new_features {
                if !existing.contains(feat) {
                    arr.push(feat.as_str());
                }
            }
        }
    } else if let Some(tbl) = ply_dep.as_table_like_mut() {
        let features_item = tbl.entry("features").or_insert(toml_edit::Item::Value(
            toml_edit::Value::Array(toml_edit::Array::new()),
        ));
        if let Some(arr) = features_item.as_array_mut() {
            let existing: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            for feat in new_features {
                if !existing.contains(feat) {
                    arr.push(feat.as_str());
                }
            }
        }
    }

    // If shader-pipeline is being added, ensure build-dependencies and build.rs exist
    if new_features.iter().any(|f| f == "shader-pipeline") {
        // Add [build-dependencies] if not present
        if doc.get("build-dependencies").is_none() {
            doc["build-dependencies"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let build_deps = &mut doc["build-dependencies"]["ply-engine"];
        if build_deps.is_none() {
            let mut tbl = toml_edit::InlineTable::new();
            tbl.insert(
                "git",
                toml_edit::Value::from("https://github.com/TheRedDeveloper/ply-engine"),
            );
            let mut arr = toml_edit::Array::new();
            arr.push("shader-build");
            tbl.insert("features", toml_edit::Value::Array(arr));
            *build_deps = toml_edit::Item::Value(toml_edit::Value::InlineTable(tbl));
        }

        // Create build.rs if it doesn't exist
        if !Path::new("build.rs").exists() {
            fs::write("build.rs", templates::BUILD_RS)
                .map_err(|e| format!("Failed to write build.rs: {e}"))?;
            println!("  Created build.rs");
        }

        // Create shaders/ directory
        fs::create_dir_all("shaders")
            .map_err(|e| format!("Failed to create shaders/: {e}"))?;
    }

    fs::write("Cargo.toml", doc.to_string())
        .map_err(|e| format!("Failed to write Cargo.toml: {e}"))?;

    Ok(())
}

/// Detect fonts already present in assets/fonts/ (by filename → font name).
fn detect_installed_fonts() -> Vec<String> {
    let fonts_dir = Path::new("assets/fonts");
    if !fonts_dir.exists() {
        return Vec::new();
    }

    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir(fonts_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("ttf") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    // Convert filename back to title case: "open_sans" → "Open Sans"
                    let name = stem
                        .split('_')
                        .map(|word| {
                            let mut chars = word.chars();
                            match chars.next() {
                                Some(c) => {
                                    c.to_uppercase().to_string() + &chars.collect::<String>()
                                }
                                None => String::new(),
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    names.push(name);
                }
            }
        }
    }
    names
}
