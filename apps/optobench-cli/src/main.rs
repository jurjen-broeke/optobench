//! The `optobench` command-line interface.
//!
//! Every command takes JSON parameters and returns a JSON result, the same
//! shape the UI, a later MCP server or a SolidCraft plugin will call.
//! M0 ships only `version` and `help`; commands arrive with their milestones.

#![forbid(unsafe_code)]

use std::process::ExitCode;

const HELP: &str = "\
optobench: optics bench for light paths in CAD assemblies and planar layouts

Usage: optobench <command>

Commands:
  version    print the application and file-format versions
  help       print this help

Planned (see docs/design.md, Milestones):
  trace, optimize, fetch, export-placements
";

fn main() -> ExitCode {
    let command = std::env::args().nth(1).unwrap_or_else(|| "help".to_owned());
    match command.as_str() {
        "version" | "--version" | "-V" => {
            println!(
                "optobench {} (file format {})",
                env!("CARGO_PKG_VERSION"),
                optobench_io::format_tag()
            );
            ExitCode::SUCCESS
        }
        "help" | "--help" | "-h" => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown command `{other}`; run `optobench help`");
            ExitCode::FAILURE
        }
    }
}
