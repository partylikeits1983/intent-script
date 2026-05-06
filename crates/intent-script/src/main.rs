use std::path::PathBuf;
use std::process;

use clap::Parser;

use intent_script::output::CompileOutputJson;

#[derive(Parser)]
#[command(name = "intent-script")]
#[command(about = "Compile intent-script JSON into unsigned EVM transactions")]
struct Cli {
    /// Path to the intent JSON file
    input: PathBuf,

    /// Path to the config directory (default: ./config)
    #[arg(short, long, default_value = "./config")]
    config_dir: PathBuf,

    /// Pretty-print the JSON output
    #[arg(short, long)]
    pretty: bool,

    /// Emit machine-readable structured errors alongside the human prose.
    /// On compile failure, prints `INTENT_ERROR_V1::{json}` to stderr (same
    /// shape the WASM binding produces) so agentic CLI flows can branch on
    /// `code` and apply `fixInstruction` mechanically.
    #[arg(long)]
    structured: bool,
}

fn main() {
    let cli = Cli::parse();

    let json_input = match std::fs::read_to_string(&cli.input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", cli.input.display(), e);
            process::exit(1);
        }
    };

    // Load config files and pass as strings to the library
    let (chains_json, assets_json, protocols_json) = match load_config(&cli.config_dir, &json_input)
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Config error: {e}");
            process::exit(1);
        }
    };

    match intent_script::compile(&json_input, &chains_json, &assets_json, &protocols_json) {
        Ok(result) => {
            // Print warnings to stderr
            for warning in &result.warnings {
                eprintln!("⚠ Warning: {warning}");
            }

            let json_output = CompileOutputJson::from(&result);
            let result = if cli.pretty {
                serde_json::to_string_pretty(&json_output)
            } else {
                serde_json::to_string(&json_output)
            };

            match result {
                Ok(s) => println!("{s}"),
                Err(e) => {
                    eprintln!("Error serializing output: {e}");
                    process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Compile error: {e}");
            if cli.structured {
                let s = e.to_structured();
                match serde_json::to_string(&s) {
                    Ok(json) => eprintln!("INTENT_ERROR_V1::{json}"),
                    Err(serr) => eprintln!("INTENT_ERROR_V1::serialize_failed: {serr}"),
                }
            }
            process::exit(1);
        }
    }
}

/// Load config files from the config directory.
/// Extracts the network from the JSON input to determine which asset/protocol files to load.
fn load_config(config_dir: &PathBuf, json_input: &str) -> Result<(String, String, String), String> {
    // Parse just the network field from the input
    #[derive(serde::Deserialize)]
    struct NetworkOnly {
        network: String,
    }
    let parsed: NetworkOnly =
        serde_json::from_str(json_input).map_err(|e| format!("Failed to parse input JSON: {e}"))?;
    let network = &parsed.network;

    let chains_path = config_dir.join("chains.json");
    let chains = std::fs::read_to_string(&chains_path)
        .map_err(|e| format!("Failed to read {}: {e}", chains_path.display()))?;

    let assets_path = config_dir.join("assets").join(format!("{network}.json"));
    let assets = std::fs::read_to_string(&assets_path)
        .map_err(|e| format!("Failed to read {}: {e}", assets_path.display()))?;

    let protocols_path = config_dir.join("protocols").join(format!("{network}.json"));
    let protocols = std::fs::read_to_string(&protocols_path)
        .map_err(|e| format!("Failed to read {}: {e}", protocols_path.display()))?;

    Ok((chains, assets, protocols))
}
