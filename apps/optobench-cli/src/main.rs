//! The `optobench` command-line interface.
//!
//! A thin shell over the command registry (`optobench-commands`): every command takes
//! JSON parameters and returns a JSON result, the same calls the UI, a later MCP server
//! or a SolidCraft plugin make.
//!
//! ```text
//! optobench <command> [parameters]
//! ```
//!
//! `parameters` is a JSON object, or `-` to read it from standard input. The result is
//! printed to standard output as JSON. A failure prints `{"error": {...}}` to standard
//! output as well, so callers read one stream, and exits with status 1.

#![forbid(unsafe_code)]

use std::io::Read;
use std::process::ExitCode;

use optobench_commands::{CommandError, ErrorCode, Registry};
use serde_json::Value;

/// Commands designed but not yet built (docs/design.md, Milestones).
const PLANNED: &str = "optimize";

fn help(registry: &Registry) -> String {
    let mut text = String::from(
        "optobench: optics bench for light paths in CAD assemblies and planar layouts\n\n\
         Usage: optobench <command> [parameters]\n\n\
         parameters is a JSON object, or - to read it from standard input.\n\
         Every command prints its result as JSON.\n\nCommands:\n",
    );
    for command in registry.commands() {
        text.push_str(&format!("  {:<12} {}\n", command.id, command.summary));
    }
    text.push_str(&format!(
        "  {:<12} the live link: a sidecar traced against the design open in SolidCraft, again\n               after each change, one JSON line per trace ({{\"path\": \"<sidecar>\"}})\n",
        "live"
    ));
    text.push_str(&format!(
        "  {:<12} print this help\n\nPlanned (see docs/design.md, Milestones):\n  {PLANNED}\n",
        "help"
    ));
    text
}

fn read_parameters(argument: Option<&str>) -> Result<Value, CommandError> {
    let text = match argument {
        None => return Ok(Value::Null),
        Some("-") => {
            let mut buffer = String::new();
            std::io::stdin().read_to_string(&mut buffer).map_err(|e| {
                CommandError::new(
                    ErrorCode::InvalidParameters,
                    format!("could not read parameters from standard input: {e}"),
                )
            })?;
            buffer
        }
        Some(text) => text.to_owned(),
    };
    serde_json::from_str(&text).map_err(|e| {
        CommandError::new(
            ErrorCode::InvalidParameters,
            format!("parameters are not valid JSON: {e}"),
        )
    })
}

fn print_json(value: &Value) {
    match serde_json::to_string_pretty(value) {
        Ok(text) => println!("{text}"),
        Err(e) => eprintln!("could not write the result as JSON: {e}"),
    }
}

/// A trace from the live link, short: the revision, how long it took, the ledger, the
/// clipping and each detector's power and spot.
fn live_summary(update: &Value) -> Value {
    let trace = &update["trace"];
    serde_json::json!({
        "revision": update["revision"],
        "seconds": update["seconds"],
        "ledger": trace["ledger"],
        "clipping": trace["clipping"],
        "detectors": trace["detectors"].as_array().into_iter().flatten().map(|d| serde_json::json!({
            "id": d["id"],
            "power_w": d["power_w"],
            "direct_centroid_mm": d["direct_centroid_mm"],
            "direct_rms_radius_mm": d["direct_rms_radius_mm"],
        })).collect::<Vec<_>>(),
        "warnings": trace["warnings"],
    })
}

/// `optobench live {"path": "<sidecar>", …}`: the sidecar traced against the design open
/// in SolidCraft, then again after each change, one JSON line per trace (the short form,
/// or the whole `live-trace` result with `"full": true`); `"changes": n` stops after n
/// changes. Ends with the connection, or Ctrl-C.
fn live(argument: Option<&str>) -> ExitCode {
    let fail = |error: CommandError| {
        print_json(&error.to_json());
        ExitCode::FAILURE
    };
    let parameters = match read_parameters(argument) {
        Ok(Value::Object(map)) => map,
        Ok(_) => {
            return fail(CommandError::new(
                ErrorCode::InvalidParameters,
                "live takes {\"path\": \"<sidecar>\", …}",
            ));
        }
        Err(error) => return fail(error),
    };
    let mut parameters = parameters;
    let full = parameters
        .remove("full")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let changes = parameters.remove("changes").and_then(|v| v.as_u64());
    let show = |update: &Value| {
        let shown = if full {
            update.clone()
        } else {
            live_summary(update)
        };
        println!("{shown}");
    };
    let (mut session, first) = match optobench_bridge::live::Live::start(&parameters) {
        Ok(started) => started,
        Err(error) => return fail(error),
    };
    show(&first);
    let mut seen = 0;
    while changes.is_none_or(|n| seen < n) {
        match session.next(std::time::Duration::from_secs(3600)) {
            Ok(Some(update)) => {
                show(&update);
                seen += 1;
            }
            Ok(None) => {}
            Err(error) => return fail(error),
        }
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let registry = Registry::standard();
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let command = arguments.first().map(String::as_str).unwrap_or("help");

    match command {
        "help" | "--help" | "-h" => {
            print!("{}", help(&registry));
            return ExitCode::SUCCESS;
        }
        // The conventional one-line version for people; `optobench version` gives JSON.
        "--version" | "-V" => {
            let result = registry.run("version", &Value::Null).unwrap_or_default();
            println!(
                "optobench {} (file format {})",
                result["version"].as_str().unwrap_or("?"),
                result["file_format"].as_str().unwrap_or("?")
            );
            return ExitCode::SUCCESS;
        }
        _ => {}
    }

    if arguments.len() > 2 {
        let error = CommandError::new(
            ErrorCode::InvalidParameters,
            "pass the parameters as one JSON object (quote it), or - to read standard input",
        );
        print_json(&error.to_json());
        return ExitCode::FAILURE;
    }

    if command == "live" {
        return live(arguments.get(1).map(String::as_str));
    }

    let outcome = read_parameters(arguments.get(1).map(String::as_str))
        .and_then(|parameters| registry.run(command, &parameters));
    match outcome {
        Ok(result) => {
            print_json(&result);
            ExitCode::SUCCESS
        }
        Err(error) => {
            print_json(&error.to_json());
            ExitCode::FAILURE
        }
    }
}
