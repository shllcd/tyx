use std::process::{Command, ExitCode};

const USAGE: &str = "usage: cargo xtask bench [args...]";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    if command != "bench" {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    }
    match Command::new("cargo").args(["run", "--release", "--package", "tyx-bench", "--"]).args(args).status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("failed to run cargo: {error}");
            ExitCode::FAILURE
        }
    }
}
