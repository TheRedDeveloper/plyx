use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const SKILL_TEXT: &str = include_str!("../SKILL.md");

pub(crate) fn home_dir() -> Result<PathBuf, String> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| "Could not resolve HOME environment variable.".to_string())
}

pub(crate) fn user_skill_file() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".claude/skills/ply-engine/SKILL.md"))
}

pub(crate) fn project_skill_file(project_root: &Path) -> PathBuf {
    project_root.join(".claude/skills/ply-engine/SKILL.md")
}

pub(crate) fn install_user_skill() -> Result<PathBuf, String> {
    let skill_file = user_skill_file()?;
    install_skill_to_path(&skill_file)?;
    Ok(skill_file)
}

pub(crate) fn install_project_skill(project_root: &Path) -> Result<PathBuf, String> {
    let skill_file = project_skill_file(project_root);
    install_skill_to_path(&skill_file)?;
    Ok(skill_file)
}

fn install_skill_to_path(skill_file: &Path) -> Result<(), String> {
    if let Some(parent) = skill_file.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create skill directory '{}': {e}", parent.display()))?;
    }

    fs::write(skill_file, SKILL_TEXT)
        .map_err(|e| format!("Failed to write skill file '{}': {e}", skill_file.display()))
}
