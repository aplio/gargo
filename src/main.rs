use clap::Parser;
use gargo::config::Config;
use gargo::core::editor::Editor;

fn main() {
    let cli = gargo::cli::Cli::parse();
    match cli.mode() {
        gargo::cli::CliMode::CheckUpgrade => {
            match gargo::upgrade::run(gargo::upgrade::UpgradeCommand::Check) {
                Ok(message) => {
                    println!("{message}");
                    return;
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        gargo::cli::CliMode::Upgrade => {
            match gargo::upgrade::run(gargo::upgrade::UpgradeCommand::Upgrade) {
                Ok(message) => {
                    println!("{message}");
                    return;
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        gargo::cli::CliMode::RunEditor => {}
    }

    let config = Config::load();
    let path_arg = cli.path.as_ref().and_then(|p| p.to_str());

    let editor = match path_arg {
        Some(path) => {
            let p = std::path::Path::new(path);
            if p.is_dir() {
                Editor::new()
            } else {
                Editor::open(path)
            }
        }
        None => Editor::new(),
    };

    let start_path = path_arg.map(std::path::Path::new);
    let mut app = gargo::app::App::new(editor, config, start_path);
    let mut stdout = gargo::terminal::setup();
    let result = app.run(&mut stdout);
    gargo::terminal::teardown(stdout);

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
