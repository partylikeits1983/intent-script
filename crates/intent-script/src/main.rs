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

    match intent_script::compile(&json_input, &cli.config_dir) {
        Ok(output) => {
            let json_output = CompileOutputJson::from(&output);
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
            process::exit(1);
        }
    }
}
