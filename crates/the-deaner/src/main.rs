use clap::Parser;

fn main() {
    if let Err(e) = the_deaner::cli::run(the_deaner::cli::Cli::parse()) {
        eprintln!("the-deaner: {e:#}");
        std::process::exit(1);
    }
}
