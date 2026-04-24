use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

#[derive(Debug)]
pub enum CompileError {
    UnknownNetwork(String),
    UnknownAsset {
        asset: String,
        network: String,
        suggestion: Option<String>,
    },
    UnknownProtocol {
        protocol: String,
        network: String,
        available: Vec<String>,
    },
    InvalidAmount(String),
    InvalidAddress(String),
    Config(String),
    UnsupportedStep(String),
    Validation(String),
    InsufficientBalance {
        token: String,
        required: String,
        available: String,
    },
    SlippageTooLow {
        step_index: usize,
        current: String,
    },
    HealthFactorRisk {
        current: f64,
        threshold: f64,
    },
    InvalidChain(String),
    Adapter(String),
    Json(String),
    /// A batched intent was compiled without any source for an EIP-712
    /// deadline. The on-chain router's `executeSigned` rejects
    /// `deadline == 0`, so we refuse to emit such an intent at compile
    /// time instead of producing a tx that is guaranteed to revert.
    DeadlineMissing,
    /// An explicit `deadline` is at or before the supplied
    /// `current_timestamp`. The signed intent would be rejected by the
    /// router; fail fast at compile time.
    DeadlineInPast {
        deadline: u64,
        current_timestamp: u64,
    },
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::UnknownNetwork(s) => write!(f, "Unknown network: {s}"),
            CompileError::UnknownAsset {
                asset,
                network,
                suggestion,
            } => match suggestion {
                Some(s) => write!(
                    f,
                    "Unknown asset '{asset}' on network '{network}'. Did you mean '{s}'?"
                ),
                None => write!(f, "Unknown asset '{asset}' on network '{network}'"),
            },
            CompileError::UnknownProtocol {
                protocol,
                network,
                available,
            } => {
                if available.is_empty() {
                    write!(f, "Unknown protocol '{protocol}' on network '{network}'")
                } else {
                    write!(
                        f,
                        "Unknown protocol '{protocol}' on network '{network}'. Available: {}",
                        available.join(", ")
                    )
                }
            }
            CompileError::InvalidAmount(s) => write!(f, "Invalid amount: {s}"),
            CompileError::InvalidAddress(s) => write!(f, "Invalid address: {s}"),
            CompileError::Config(s) => write!(f, "Config error: {s}"),
            CompileError::UnsupportedStep(s) => write!(f, "Unsupported step: {s}"),
            CompileError::Validation(s) => write!(f, "Validation error: {s}"),
            CompileError::InsufficientBalance {
                token,
                required,
                available,
            } => write!(
                f,
                "Insufficient {token} balance: need {required}, have {available}"
            ),
            CompileError::SlippageTooLow {
                step_index,
                current,
            } => write!(
                f,
                "Step {step_index}: slippage protection too low (current minimum: {current})"
            ),
            CompileError::HealthFactorRisk { current, threshold } => write!(
                f,
                "Aave health factor {current:.2} is below minimum {threshold:.1}; borrow rejected to prevent liquidation"
            ),
            CompileError::InvalidChain(s) => write!(f, "Invalid intent chain: {s}"),
            CompileError::Adapter(s) => write!(f, "Adapter error: {s}"),
            CompileError::Json(s) => write!(f, "JSON parse error: {s}"),
            CompileError::DeadlineMissing => write!(
                f,
                "Batched intent has no deadline: neither 'deadline' nor 'current_timestamp' \
                 was provided. The router's executeSigned rejects deadline == 0; set \
                 'current_timestamp' to the current Unix timestamp to auto-compute a \
                 30-minute deadline, or pass an explicit 'deadline' > current_timestamp."
            ),
            CompileError::DeadlineInPast {
                deadline,
                current_timestamp,
            } => write!(
                f,
                "Intent 'deadline' {deadline} is at or before 'current_timestamp' \
                 {current_timestamp}; the router would reject this signed intent. \
                 Use a 'deadline' strictly greater than the current timestamp."
            ),
        }
    }
}

/// Compute a suggestion for an unknown key by picking the closest match from
/// a list of candidates using case-insensitive Levenshtein distance.
///
/// Returns None if no candidate is within distance 3 (avoids nonsense suggestions).
pub fn closest_match<'a, I>(input: &str, candidates: I) -> Option<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let input_lower = input.to_lowercase();
    let mut best: Option<(usize, String)> = None;

    for candidate in candidates {
        let dist = levenshtein(&input_lower, &candidate.to_lowercase());
        match &best {
            Some((best_dist, _)) if dist >= *best_dist => {}
            _ => best = Some((dist, candidate.to_string())),
        }
    }

    best.and_then(|(dist, name)| if dist <= 3 { Some(name) } else { None })
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr: Vec<usize> = alloc::vec![0; m + 1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        core::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

impl From<serde_json::Error> for CompileError {
    fn from(e: serde_json::Error) -> Self {
        CompileError::Json(format!("{e}"))
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CompileError {}

pub type Result<T> = core::result::Result<T, CompileError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_close_match_for_typo() {
        let candidates = ["USDC", "USDT", "WETH", "DAI"];
        let suggestion = closest_match("UDSC", candidates.iter().copied());
        assert_eq!(suggestion.as_deref(), Some("USDC"));
    }

    #[test]
    fn no_suggestion_when_far_from_all() {
        let candidates = ["USDC", "WETH", "DAI"];
        let suggestion = closest_match("RANDOM_GARBAGE_TOKEN", candidates.iter().copied());
        assert_eq!(suggestion, None);
    }

    #[test]
    fn unknown_asset_display_includes_suggestion() {
        let err = CompileError::UnknownAsset {
            asset: "UDSC".into(),
            network: "ethereum".into(),
            suggestion: Some("USDC".into()),
        };
        let msg = alloc::format!("{err}");
        assert!(msg.contains("Did you mean 'USDC'"));
    }

    #[test]
    fn unknown_protocol_display_lists_available() {
        let err = CompileError::UnknownProtocol {
            protocol: "foo".into(),
            network: "ethereum".into(),
            available: alloc::vec!["aave".into(), "uniswap".into()],
        };
        let msg = alloc::format!("{err}");
        assert!(msg.contains("Available: aave, uniswap"));
    }
}
