use crate::fonts;
use crate::templates::*;
use crate::tui;
use std::fs;
use std::path::Path;

pub fn run() {
    if let Err(e) = run_inner() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_inner() -> Result<(), String> {
    let name = tui::text_input("Project name:", "my-app")?;

    let project_dir = Path::new(&name);
    if project_dir.exists() {
        return Err(format!("Directory '{name}' already exists."));
    }

    // Font selection
    let font_list = fonts::load_font_list()?;

    let mut options: Vec<String> = Vec::new();
    for &suggested in fonts::SUGGESTED_FONTS {
        if suggested == fonts::DEFAULT_FONT {
            options.push(format!("{suggested} (Default)"));
        } else {
            options.push(suggested.to_string());
        }
    }
    // Add the rest of the catalog (skip duplicates with suggested list)
    for font in &font_list {
        if !fonts::SUGGESTED_FONTS
            .iter()
            .any(|&s| s.eq_ignore_ascii_case(font))
        {
            options.push(font.clone());
        }
    }

    let selected_label = tui::search_select(
        "Choose your first font:",
        &options,
        "Don't worry, you can add more fonts later with `plyx add`",
    )?;

    let font_name = selected_label
        .strip_suffix(" (Default)")
        .unwrap_or(&selected_label);

    let resolved_font = if fonts::SUGGESTED_FONTS.contains(&font_name) {
        font_name.to_string()
    } else {
        fonts::find_by_name(&font_list, font_name)
            .map(|s| s.to_string())
            .unwrap_or_else(|| font_name.to_string())
    };

    // Feature selection
    let enabled_keys = tui::feature_select(
        "Select features (space to select, arrow-keys to navigate):",
        FEATURES,
        "Don't worry, you can activate these later with `plyx add`",
        &[],  // no pre-checked
        &[],  // no locked
        "Create!",
    )?;

    let enabled_refs: Vec<&str> = enabled_keys.iter().map(|s| s.as_str()).collect();
    let has_shader_pipeline = enabled_refs.contains(&"shader-pipeline");

    println!("\nCreating project '{name}'...");

    fs::create_dir_all(project_dir.join("src"))
        .map_err(|e| format!("Failed to create directories: {e}"))?;
    fs::create_dir_all(project_dir.join("assets/fonts"))
        .map_err(|e| format!("Failed to create assets/fonts: {e}"))?;

    if has_shader_pipeline {
        fs::create_dir_all(project_dir.join("shaders"))
            .map_err(|e| format!("Failed to create shaders/: {e}"))?;
    }

    fonts::download(&resolved_font, &project_dir.join("assets/fonts"))?;
    let font_filename = resolved_font.to_lowercase().replace(' ', "_") + ".ttf";

    let cargo_toml = generate_cargo_toml(&name, &enabled_refs);
    fs::write(project_dir.join("Cargo.toml"), cargo_toml)
        .map_err(|e| format!("Failed to write Cargo.toml: {e}"))?;

    let main_rs = generate_main_rs(&font_filename);
    fs::write(project_dir.join("src/main.rs"), main_rs)
        .map_err(|e| format!("Failed to write src/main.rs: {e}"))?;

    if has_shader_pipeline {
        fs::write(project_dir.join("build.rs"), BUILD_RS)
            .map_err(|e| format!("Failed to write build.rs: {e}"))?;
    }

    fs::write(project_dir.join(".gitignore"), "/target\n/build\n")
        .map_err(|e| format!("Failed to write .gitignore: {e}"))?;

    println!("\nProject '{name}' created!");
    println!("  cd {name}");
    println!("  cargo run");

    Ok(())
}