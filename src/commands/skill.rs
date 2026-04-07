use crate::skill;

pub fn run(install: bool) {
    if install {
        match skill::install_user_skill() {
            Ok(path) => {
                println!("Installed skill to {}", path.display());
            }
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    } else {
        print!("{}", skill::SKILL_TEXT);
    }
}
