# Config Files & Registry System

> **Load this file** when you need to understand how config files work, how to add new chains/assets/protocols, or how the registry loads and resolves data.

## Config Directory Structure

```
config/
├── chains.json              # All supported chains
├── assets/
│   └── ethereum.json        # Token addresses + decimals for Ethereum mainnet
└── protocols/
    └── ethereum.json         # Protocol contract addresses for Ethereum mainnet
```

The CLI reads these files and passes their contents as strings to the library's `compile()` function. The library does no file I/O.

---

## `chains.json` — Chain Definitions

Maps network aliases to chain metadata.

```json
{
  "ethereum": {
    "chain_id": 1,
    "native_asset": "ETH",
    "wrapped_native": "WETH"
  },
  "sepolia": {
    "chain_id": 11155111,
    "native_asset": "ETH",
    "wrapped_native": "WETH"
  },
  "base": {
    "chain_id": 8453,
    "native_asset": "ETH",
    "wrapped_native": "WETH"
  },
  "arbitrum": {
    "chain_id": 42161,
    "native_asset": "ETH",
    "wrapped_native": "WETH"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `chain_id` | number | EVM chain ID |
| `native_asset` | string | Alias for the native gas token (e.g., `"ETH"`) |
| `wrapped_native` | string | Alias for the wrapped native token (e.g., `"WETH"`) |

**Serde type:** `ChainConfig` in `crates/intent-script/src/registry/loader.rs:13`

### Adding a New Chain

1. Add an entry to `config/chains.json`
2. Create `config/assets/{network}.json` with token definitions
3. Create `config/protocols/{network}.json` with protocol addresses
4. The compiler will automatically support the new network when the JSON input specifies `"network": "{network}"`

---

## `assets/{network}.json` — Token Definitions

Maps token aliases to addresses and decimals.

```json
{
  "ETH": {
    "address": "native",
    "decimals": 18
  },
  "WETH": {
    "address": "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2",
    "decimals": 18
  },
  "USDC": {
    "address": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
    "decimals": 6
  },
  "USDT": {
    "address": "0xdAC17F958D2ee523a2206206994597C13D831ec7",
    "decimals": 6
  },
  "DAI": {
    "address": "0x6B175474E89094C44Da98b954EedeAC495271d0F",
    "decimals": 18
  },
  "WBTC": {
    "address": "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599",
    "decimals": 8
  },
  "stETH": {
    "address": "0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84",
    "decimals": 18
  },
  "wstETH": {
    "address": "0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0",
    "decimals": 18
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `address` | string | `"native"` for the chain's native asset, or a hex address (checksummed) |
| `decimals` | number | Token decimal places (ETH/WETH=18, USDC/USDT=6, WBTC=8) |

**Serde type:** `AssetConfig` in `crates/intent-script/src/registry/loader.rs:20`

### Adding a New Token

Add an entry to the appropriate `config/assets/{network}.json`:

```json
{
  "MY_TOKEN": {
    "address": "0x1234567890abcdef1234567890abcdef12345678",
    "decimals": 18
  }
}
```

The token alias (`"MY_TOKEN"`) can then be used in JSON intents: `{ "swap": { "from": "MY_TOKEN", ... } }`.

**Special value:** `"address": "native"` marks the chain's native gas token. The registry's `is_native()` method checks for this.

---

## `protocols/{network}.json` — Protocol Definitions

Maps protocol aliases to contract addresses.

```json
{
  "aave": {
    "type": "lending",
    "version": "v3",
    "contracts": {
      "pool": "0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2"
    }
  },
  "uniswap": {
    "type": "dex",
    "version": "v3",
    "contracts": {
      "router": "0xE592427A0AEce92De3Edee1F18E0157C05861564",
      "quoter": "0xb27308f9F90D607463bb33eA1BeBb41C27CE5AB6"
    }
  },
  "lido": {
    "type": "staking",
    "version": "v1",
    "contracts": {
      "steth": "0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84",
      "wsteth": "0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0"
    }
  },
  "1inch": {
    "type": "dex_aggregator",
    "version": "v6",
    "contracts": {
      "router": "0x111111125421cA6dc452d289314280a0f8842A65"
    }
  },
  "intent_router": {
    "type": "router",
    "version": "v1",
    "contracts": {
      "router": "0x1111111254EEB25477B68fb85Ed929f73A960582"
    }
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `type` | string | Protocol category: `"lending"`, `"dex"`, `"staking"`, `"dex_aggregator"`, `"router"` |
| `version` | string | Protocol version (e.g., `"v3"`) |
| `contracts` | object | Map of contract name → hex address |

**Serde type:** `ProtocolConfig` in `crates/intent-script/src/registry/loader.rs:28`

### Adding a New Protocol

1. Add an entry to `config/protocols/{network}.json`:
   ```json
   {
     "my_protocol": {
       "type": "lending",
       "version": "v1",
       "contracts": {
         "pool": "0x..."
       }
     }
   }
   ```

2. The normalizer accesses it via `registry.protocols.get("my_protocol")` and then `config.contracts.get("pool")`

### Special Protocol: `intent_router`

The `intent_router` entry is special — it provides the IntentRouter contract address used for batching. The registry's `router_address()` method looks up `protocols["intent_router"].contracts["router"]`.

If no `intent_router` is configured (or the address is zero), the compiler falls back to `TxSequence` mode instead of `Batched`.

---

## RegistryContext API

The `RegistryContext` struct in `crates/intent-script/src/registry/loader.rs:38` provides the lookup interface:

```rust
pub struct RegistryContext {
    pub network: String,
    pub chain: ChainConfig,
    pub assets: HashMap<String, AssetConfig>,
    pub protocols: HashMap<String, ProtocolConfig>,
}
```

### Loading

```rust
// The library accepts pre-loaded JSON strings (no file I/O)
let registry = RegistryContext::load(
    chains_json,      // contents of config/chains.json
    assets_json,      // contents of config/assets/ethereum.json
    protocols_json,   // contents of config/protocols/ethereum.json
    "ethereum",       // network name
)?;
```

### Key Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `is_native(alias)` | `bool` | True if alias is the native asset (e.g., `"ETH"`) |
| `is_wrapped_native(alias)` | `bool` | True if alias is the wrapped native (e.g., `"WETH"`) |
| `router_address()` | `Option<Address>` | IntentRouter address, or `None` if not configured |

### Common Lookup Patterns in Normalizer

The normalizer in `crates/intent-script/src/compiler/normalize.rs` uses these patterns:

```rust
// Look up asset address
let asset_config = registry.assets.get(&alias)
    .ok_or_else(|| CompileError::UnknownAsset { asset: alias.clone(), network: registry.network.clone() })?;
let address: Address = asset_config.address.parse()
    .map_err(|_| CompileError::InvalidAddress(...))?;
let decimals = asset_config.decimals;

// Look up protocol contract
let protocol = registry.protocols.get("aave")
    .ok_or_else(|| CompileError::UnknownProtocol { protocol: "aave".into(), network: registry.network.clone() })?;
let pool: Address = protocol.contracts.get("pool")
    .ok_or_else(|| CompileError::Config("Missing 'pool' contract for aave".into()))?
    .parse()
    .map_err(|_| CompileError::InvalidAddress(...))?;

// Check if native
if registry.is_native(&alias) {
    // Use Address::ZERO for native ETH
}
```

---

## How Config Flows Through the Pipeline

```
CLI (main.rs)
  ├── reads config/chains.json → chains_json: &str
  ├── reads config/assets/{network}.json → assets_json: &str
  └── reads config/protocols/{network}.json → protocols_json: &str
        │
        ▼
compile(json_input, chains_json, assets_json, protocols_json)
        │
        ▼
RegistryContext::load(chains_json, assets_json, protocols_json, network)
        │
        ▼
normalize(&script, &registry)  ← uses registry to resolve aliases
validate(&resolved, &registry) ← uses registry for protocol checks
enrich(resolved, &registry)    ← uses registry for router address
lower(&enriched, &registry)    ← adapters may need registry
plan(&calls, router, sweeps)   ← router address from registry
build(plan, chain_id, ...)     ← chain_id from registry
```

---

## Test Config Helper

In tests, config is loaded from the workspace `config/` directory:

```rust
fn config_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()  // crates/
        .parent().unwrap()  // project root
        .join("config")
}

fn load_config() -> (String, String, String) {
    let dir = config_dir();
    let chains = std::fs::read_to_string(dir.join("chains.json")).unwrap();
    let assets = std::fs::read_to_string(dir.join("assets/ethereum.json")).unwrap();
    let protocols = std::fs::read_to_string(dir.join("protocols/ethereum.json")).unwrap();
    (chains, assets, protocols)
}
```

This pattern is used in `crates/intent-script/tests/integration.rs` and other test files.
