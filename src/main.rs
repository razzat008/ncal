use clap::Parser;
use ncal::{app, cli::Cli, config};

fn main() {
    let cli = Cli::parse();
    if let Some(config) = config::Config::from_cli(&cli).ok() {
        if let Err(e) = config.validate() {
            eprintln!("Error: {e}");
        } else {
            app::run(config).unwrap_or_else(|e| eprintln!("Error: {e}"));
        }
    }
}
