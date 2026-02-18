use clap_complete::Shell;
use std::io::Write;
use std::process::Command;

/// Names of hidden subcommands to strip from shell completions.
const HIDDEN_COMMANDS: &[&str] = &["remove", "delete", "erase", "help"];

/// Build a filtered clap Command with hidden subcommands removed.
fn filtered_command() -> clap::Command {
    use clap::CommandFactory;
    let cmd = crate::Cli::command();
    let subcommands: Vec<clap::Command> = cmd
        .get_subcommands()
        .filter(|sub| !HIDDEN_COMMANDS.contains(&sub.get_name()))
        .cloned()
        .collect();
    let mut clean = clap::Command::new("plyx").disable_help_subcommand(true);
    for sub in subcommands {
        clean = clean.subcommand(sub);
    }
    clean
}

pub fn run(shell: Shell, install: bool) {
    let mut cmd = filtered_command();

    if install {
        install_completions(shell, &mut cmd);
    } else {
        clap_complete::generate(shell, &mut cmd, "plyx", &mut std::io::stdout());
    }
}

fn install_completions(shell: Shell, cmd: &mut clap::Command) {
    match shell {
        Shell::Zsh => install_zsh(cmd),
        Shell::Bash => install_bash(cmd),
        Shell::Fish => install_fish(cmd),
        _ => {
            eprintln!(
                "Auto-install not supported for {shell:?}. \
                 Use `plyx completions {shell:?}` to print completions and install manually."
            );
            std::process::exit(1);
        }
    }
}

/// Try writing to a file. If permission denied, retry with sudo.
fn write_with_sudo_fallback(path: &str, content: &[u8], description: &str) -> bool {
    match std::fs::write(path, content) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            println!("Permission denied writing {description}. Retrying with sudo...");
            let status = Command::new("sudo")
                .args(["tee", path])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .spawn()
                .and_then(|mut child| {
                    if let Some(ref mut stdin) = child.stdin {
                        stdin.write_all(content)?;
                    }
                    child.wait()
                });
            match status {
                Ok(s) if s.success() => true,
                _ => {
                    eprintln!("Failed to write {description} even with sudo.");
                    false
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to write {description}: {e}");
            false
        }
    }
}

/// Try appending to a file. If permission denied, retry with sudo.
fn append_with_sudo_fallback(path: &str, content: &str, description: &str) -> bool {
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        Ok(mut f) => {
            write!(f, "{content}").ok();
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            println!("Permission denied writing {description}. Retrying with sudo...");
            let status = Command::new("sudo")
                .args(["tee", "-a", path])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .spawn()
                .and_then(|mut child| {
                    if let Some(ref mut stdin) = child.stdin {
                        stdin.write_all(content.as_bytes())?;
                    }
                    child.wait()
                });
            match status {
                Ok(s) if s.success() => true,
                _ => {
                    eprintln!("Failed to write {description} even with sudo.");
                    false
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to write {description}: {e}");
            false
        }
    }
}

fn install_zsh(cmd: &mut clap::Command) {
    let home = std::env::var("HOME").expect("HOME not set");
    let zfunc_dir = format!("{home}/.zfunc");
    let comp_path = format!("{zfunc_dir}/_plyx");

    if let Err(e) = std::fs::create_dir_all(&zfunc_dir) {
        eprintln!("Could not create {zfunc_dir}: {e}");
        std::process::exit(1);
    }

    let mut buf = Vec::new();
    clap_complete::generate(Shell::Zsh, cmd, "plyx", &mut buf);

    if !write_with_sudo_fallback(&comp_path, &buf, &comp_path) {
        std::process::exit(1);
    }

    // Check if .zshrc already has fpath setup
    let zshrc_path = format!("{home}/.zshrc");
    let zshrc = std::fs::read_to_string(&zshrc_path).unwrap_or_default();

    if !zshrc.contains(".zfunc") {
        let snippet = "\n# plyx shell completions\nfpath=(~/.zfunc $fpath)\nautoload -Uz compinit && compinit\n";
        if append_with_sudo_fallback(&zshrc_path, snippet, "~/.zshrc") {
            println!("Added fpath + compinit to ~/.zshrc");
        }
    }

    println!("Installed zsh completions to {comp_path}");
    println!("Restart your shell or run: source ~/.zshrc");
}

fn install_bash(cmd: &mut clap::Command) {
    let home = std::env::var("HOME").expect("HOME not set");
    let comp_path = format!("{home}/.local/share/bash-completion/completions/plyx");

    if let Some(parent) = std::path::Path::new(&comp_path).parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let mut buf = Vec::new();
    clap_complete::generate(Shell::Bash, cmd, "plyx", &mut buf);

    if !write_with_sudo_fallback(&comp_path, &buf, &comp_path) {
        std::process::exit(1);
    }

    println!("Installed bash completions to {comp_path}");
    println!("Restart your shell to activate.");
}

fn install_fish(cmd: &mut clap::Command) {
    let home = std::env::var("HOME").expect("HOME not set");
    let comp_path = format!("{home}/.config/fish/completions/plyx.fish");

    if let Some(parent) = std::path::Path::new(&comp_path).parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let mut buf = Vec::new();
    clap_complete::generate(Shell::Fish, cmd, "plyx", &mut buf);

    if !write_with_sudo_fallback(&comp_path, &buf, &comp_path) {
        std::process::exit(1);
    }

    println!("Installed fish completions to {comp_path}");
}
