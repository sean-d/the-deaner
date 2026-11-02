use clap::Parser;

fn main() {
    if let Err(e) = deaner::cli::run(deaner::cli::Cli::parse()) {
        eprintln!("deaner: {e:#}");
        std::process::exit(1);
    }
}
