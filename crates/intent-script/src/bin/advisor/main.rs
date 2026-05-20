//! advisor — plain English → IntentScript DSL → compile → simulate, 100% in Rust.
//!
//! A native-Rust reproduction of the intentOS-ui pipeline:
//!
//!   instruction ──Rig/OpenAI──▶ DSL ──intent_script::compile──▶ unsigned txs
//!                                                └──Anvil fork──▶ asset deltas
//!
//! It exists as a debugging / full-integration-test tool: paste any L1/L2
//! wallet, type an instruction, and see what the frontend would produce — the
//! DSL, the compiled transactions, and a simulated execution — with no browser
//! and no WASM. Compile and simulation failures make the process exit non-zero,
//! so it doubles as a CI harness against the live `intent_script` compiler.
//!
//! Run:
//!   cargo run -p intent-script --features advisor --bin advisor -- \
//!     "deposit 5000 USDC into aave" --context wallet.json --pretty

mod chain;
mod config;
mod context;
mod llm;
mod parse;
mod pricing;
mod prompt;
mod simulate;

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use alloy_primitives::Address;
use clap::Parser;
use eyre::{Result, eyre};

use intent_script::output::CompileOutputJson;

use crate::context::{ContextFile, RuntimeContext};

#[derive(Parser)]
#[command(
    name = "advisor",
    about = "Natural language → IntentScript DSL → compile → simulate"
)]
struct Cli {
    /// The plain-English instruction. If omitted, read from --prompt-file or stdin.
    instruction: Option<String>,

    /// Read the instruction from a file instead of the positional argument.
    #[arg(long)]
    prompt_file: Option<PathBuf>,

    /// JSON context file: { wallet, network, balances, prices, positions }.
    #[arg(long)]
    context: Option<PathBuf>,

    /// Config directory holding chains.json + assets/ + protocols/.
    #[arg(short, long, default_value = "./config")]
    config_dir: PathBuf,

    /// OpenAI model id. Falls back to the `ADVISOR_MODEL` env var (e.g. set
    /// it in `.env`), then to `gpt-4o`. Common values: `gpt-4o`,
    /// `gpt-4o-mini`, `gpt-5-nano`, `o4-mini`.
    #[arg(long, env = "ADVISOR_MODEL", default_value = "gpt-4o")]
    model: String,

    /// Override the network from the context file (default: anvil).
    #[arg(long)]
    network: Option<String>,

    /// RPC URL — required for --fetch-balances and --simulate.
    #[arg(long)]
    rpc: Option<String>,

    /// Fetch the wallet's live native + ERC-20 balances over --rpc.
    #[arg(long)]
    fetch_balances: bool,

    /// Simulate the compiled transactions against an Anvil fork of --rpc.
    #[arg(long)]
    simulate: bool,

    /// Pretty-print JSON output.
    #[arg(short, long)]
    pretty: bool,

    /// Print the assembled system prompt and exit (no API call).
    #[arg(long)]
    dump_prompt: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("\n✗ {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Run the pipeline. `Ok(true)` = success, `Ok(false)` = a reported
/// compile/simulation failure (details already printed), `Err` = a tool error.
async fn run() -> Result<bool> {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();

    // 1. Instruction.
    let instruction = read_instruction(&cli)?;
    if instruction.trim().is_empty() {
        return Err(eyre!(
            "no instruction provided — pass it as an argument, via --prompt-file, or on stdin"
        ));
    }

    // 2. Context + network.
    let mut ctx_file = match &cli.context {
        Some(path) => ContextFile::load(path)?,
        None => ContextFile::default(),
    };
    let network = cli
        .network
        .clone()
        .or_else(|| ctx_file.network.clone())
        .unwrap_or_else(|| "anvil".to_string());
    eprintln!("→ network: {network}");

    // 3. Config.
    let cfg = config::load_config(&cli.config_dir, &network)?;
    let assets = config::parse_assets(&cfg.assets)?;

    // 4. Optional: pull the wallet's live balances and fold them into context.
    if cli.fetch_balances {
        let rpc = cli
            .rpc
            .as_deref()
            .ok_or_else(|| eyre!("--fetch-balances requires --rpc"))?;
        let wallet = ctx_file
            .wallet
            .as_deref()
            .ok_or_else(|| eyre!("--fetch-balances requires a `wallet` in the --context file"))?
            .parse::<Address>()
            .map_err(|e| eyre!("invalid wallet address in context file: {e}"))?;
        eprintln!("→ fetching live balances for {wallet} …");
        let live = chain::fetch_live_balances(rpc, wallet, &assets).await?;
        for (symbol, amount) in live {
            ctx_file.balances.insert(symbol, amount);
        }
    }

    // 5. Resolve runtime context and assemble the system prompt.
    let rt = RuntimeContext::resolve(&ctx_file, &network);
    let system_prompt = prompt::build(&rt);

    if cli.dump_prompt {
        println!("{system_prompt}");
        return Ok(true);
    }

    // 6. Ask the model (Rig → OpenAI). Time the round-trip and estimate the
    //    dollar cost so it's obvious when a slower / cheaper model is worth
    //    the swap.
    eprintln!("→ asking {} …", cli.model);
    let started = Instant::now();
    let response = llm::ask(&cli.model, &system_prompt, &instruction).await?;
    let elapsed = started.elapsed();
    let cost_str = match pricing::estimate_cost(&cli.model, &response.usage) {
        Some(cost) => format!("≈ ${cost:.5}"),
        None => "price unknown (set ADVISOR_PRICE_INPUT / ADVISOR_PRICE_OUTPUT)".to_string(),
    };
    let cached_note = if response.usage.cached_input_tokens > 0 {
        format!(" [{} cached]", response.usage.cached_input_tokens)
    } else {
        String::new()
    };
    eprintln!(
        "← {} responded in {:.2}s ({} in / {} out tokens{}, {})",
        cli.model,
        elapsed.as_secs_f64(),
        response.usage.input_tokens,
        response.usage.output_tokens,
        cached_note,
        cost_str,
    );
    let raw = response.text;

    // 7. Parse the response into title / summary / DSL JSON.
    let parsed = match parse::parse_llm_response(&raw) {
        Ok(parsed) => parsed,
        Err(e) => {
            // The model answered in Q&A mode instead of emitting an intent.
            println!("\n── Model response (no intent emitted) ──\n{}", raw.trim());
            eprintln!("\nℹ {e}");
            return Ok(true);
        }
    };

    if let Some(title) = &parsed.title {
        println!("\nTitle:   {title}");
    }
    println!("Summary: {}", parsed.summary);

    // 8. The DSL.
    println!("\n── IntentScript DSL ──");
    println!("{}", pretty_or_raw(&parsed.intent_json, cli.pretty));

    // 9. Compile.
    let result = match intent_script::compile(
        &parsed.intent_json,
        &cfg.chains,
        &cfg.assets,
        &cfg.protocols,
    ) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("\n✗ Compile error: {e}");
            return Ok(false);
        }
    };
    for warning in &result.warnings {
        eprintln!("⚠ {warning}");
    }

    println!("\n── Compiled transactions ──");
    println!("{}", to_json(&CompileOutputJson::from(&result), cli.pretty)?);

    // 10. Simulate.
    if cli.simulate {
        let rpc = cli
            .rpc
            .as_deref()
            .ok_or_else(|| eyre!("--simulate requires --rpc"))?;
        let chain_id = config::chain_id(&cfg.chains, &network)?;
        let from = intent_from(&parsed.intent_json)?;
        eprintln!("\n→ simulating on an Anvil fork of {rpc} …");
        let report = simulate::simulate(&result.output, rpc, chain_id, from, &assets).await?;
        print_sim_report(&report);
        if !report.all_ok {
            return Ok(false);
        }
    }

    Ok(true)
}

/// Resolve the instruction from the positional arg, `--prompt-file`, or stdin.
fn read_instruction(cli: &Cli) -> Result<String> {
    if let Some(text) = &cli.instruction {
        return Ok(text.clone());
    }
    if let Some(path) = &cli.prompt_file {
        return std::fs::read_to_string(path)
            .map_err(|e| eyre!("failed to read --prompt-file {}: {e}", path.display()));
    }
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| eyre!("failed to read stdin: {e}"))?;
    Ok(buf)
}

/// Pretty-print a JSON string if `pretty`, else return it unchanged.
fn pretty_or_raw(json: &str, pretty: bool) -> String {
    if !pretty {
        return json.to_string();
    }
    serde_json::from_str::<serde_json::Value>(json)
        .and_then(|v| serde_json::to_string_pretty(&v))
        .unwrap_or_else(|_| json.to_string())
}

/// Serialize a value to JSON, pretty or compact.
fn to_json<T: serde::Serialize>(value: &T, pretty: bool) -> Result<String> {
    let s = if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    };
    s.map_err(|e| eyre!("failed to serialize output: {e}"))
}

/// Extract the `from` address from an intent JSON string.
fn intent_from(intent_json: &str) -> Result<Address> {
    let value: serde_json::Value = serde_json::from_str(intent_json)?;
    let raw = value
        .get("from")
        .and_then(|f| f.as_str())
        .ok_or_else(|| eyre!("intent JSON has no string `from` field"))?;
    raw.parse::<Address>()
        .map_err(|e| eyre!("invalid `from` address '{raw}': {e}"))
}

/// Pretty-print a simulation report.
fn print_sim_report(report: &simulate::SimReport) {
    println!("\n── Simulation ──");
    for warning in &report.warnings {
        eprintln!("⚠ {warning}");
    }
    for (i, tx) in report.txs.iter().enumerate() {
        let mark = if tx.success { "✓" } else { "✗" };
        println!(
            "  {mark} tx #{}: {} (gas {})",
            i + 1,
            tx.description,
            tx.gas_used
        );
        if let Some(reason) = &tx.revert_reason {
            println!("      revert: {reason}");
        }
    }
    if report.deltas.is_empty() {
        println!("  (no balance changes detected for the signer)");
    } else {
        println!("  Asset deltas for the signer:");
        for d in &report.deltas {
            println!("    {:<8} {} → {}  ({})", d.symbol, d.before, d.after, d.change);
        }
    }
    println!(
        "  Result: {}",
        if report.all_ok {
            "all transactions succeeded"
        } else {
            "FAILED"
        }
    );
}
