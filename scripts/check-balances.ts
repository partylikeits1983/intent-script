// Diagnostic: read ETH + ERC20 balances for the 10 default anvil dev
// accounts directly from the local anvil RPC. Use this to confirm whether
// `start-anvil.sh`'s mints actually landed for the addresses the UI queries.
//
// Run:
//   node intent-script/scripts/check-balances.ts
//   node intent-script/scripts/check-balances.ts 0xYourMetaMaskAddr
//
// Requires Node 22.18+ (native TS strip). For older Node, run with
// `--experimental-strip-types`.
//
// No external deps — uses raw JSON-RPC over fetch + manual ABI encoding for
// `balanceOf(address)` (selector 0x70a08231).

const RPC = process.env.RPC_URL ?? "http://127.0.0.1:8545";

const TOKENS = {
	WETH: { addr: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", decimals: 18 },
	USDC: { addr: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", decimals: 6 },
	USDT: { addr: "0xdAC17F958D2ee523a2206206994597C13D831ec7", decimals: 6 },
	DAI: { addr: "0x6B175474E89094C44Da98b954EedeAC495271d0F", decimals: 18 },
	WBTC: { addr: "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599", decimals: 8 },
} as const;

const DEV_ACCOUNTS = [
	"0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
	"0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
	"0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC",
	"0x90F79bf6EB2c4f870365E785982E1f101E93b906",
	"0x15d34AAf54267DB7D7c367839AAf71A00a2C6A65",
	"0x9965507D1a55bcC2695C58ba16FB37d819B0A4dc",
	"0x976EA74026E726554dB657fA54763abd0C3a0aa9",
	"0x14dC79964da2C08b23698B3D3cc7Ca32193d9955",
	"0x23618e81E3f5cdF7f54C3d65f7FBc0aBf5B21E8f",
	"0xa0Ee7A142d267C1f36714E4a8F75612F20a79720",
];

const BALANCE_OF_SELECTOR = "0x70a08231";

let rpcId = 1;
async function rpc<T = unknown>(method: string, params: unknown[]): Promise<T> {
	const res = await fetch(RPC, {
		method: "POST",
		headers: { "content-type": "application/json" },
		body: JSON.stringify({ jsonrpc: "2.0", id: rpcId++, method, params }),
	});
	const body = (await res.json()) as { result?: T; error?: { message: string } };
	if (body.error) throw new Error(`${method}: ${body.error.message}`);
	return body.result as T;
}

function padAddress(addr: string): string {
	return addr.toLowerCase().replace(/^0x/, "").padStart(64, "0");
}

async function getEthBalance(addr: string): Promise<bigint> {
	const hex = await rpc<string>("eth_getBalance", [addr, "latest"]);
	return BigInt(hex);
}

async function getErc20Balance(token: string, holder: string): Promise<bigint> {
	const data = BALANCE_OF_SELECTOR + padAddress(holder);
	const hex = await rpc<string>("eth_call", [
		{ to: token, data },
		"latest",
	]);
	if (!hex || hex === "0x") return 0n;
	return BigInt(hex);
}

function format(units: bigint, decimals: number): string {
	if (units === 0n) return "0";
	const base = 10n ** BigInt(decimals);
	const whole = units / base;
	const frac = units % base;
	if (frac === 0n) return whole.toString();
	const fracStr = frac.toString().padStart(decimals, "0").replace(/0+$/, "");
	return `${whole}.${fracStr}`;
}

async function main() {
	const extras = process.argv.slice(2);
	const accounts = [...DEV_ACCOUNTS, ...extras];

	let chainId: bigint;
	let blockNumber: bigint;
	try {
		chainId = BigInt(await rpc<string>("eth_chainId", []));
		blockNumber = BigInt(await rpc<string>("eth_blockNumber", []));
	} catch (e) {
		console.error(`✗ RPC unreachable at ${RPC}: ${(e as Error).message}`);
		console.error("  Is anvil running? (intent-script/scripts/start-anvil.sh)");
		process.exit(1);
	}

	console.log(`RPC ${RPC}`);
	console.log(`chainId ${chainId}  block ${blockNumber}`);
	console.log("");

	const symbols = Object.keys(TOKENS);
	const header = ["account".padEnd(44), "ETH".padStart(12)];
	for (const s of symbols) header.push(s.padStart(14));
	console.log(header.join("  "));
	console.log("-".repeat(header.join("  ").length));

	for (const acct of accounts) {
		const eth = await getEthBalance(acct);
		const row: string[] = [acct, format(eth, 18).padStart(12)];
		for (const sym of symbols) {
			const t = TOKENS[sym as keyof typeof TOKENS];
			try {
				const bal = await getErc20Balance(t.addr, acct);
				row.push(format(bal, t.decimals).padStart(14));
			} catch (e) {
				row.push("ERR".padStart(14));
			}
		}
		console.log(row.join("  "));
	}

	// Verdict
	console.log("");
	const firstAcct = accounts[0];
	const usdc = await getErc20Balance(TOKENS.USDC.addr, firstAcct);
	const usdt = await getErc20Balance(TOKENS.USDT.addr, firstAcct);
	if (usdc === 0n && usdt === 0n) {
		console.log(
			`✗ Account ${firstAcct} has 0 USDC and 0 USDT. The mint in start-anvil.sh did not land.`,
		);
		console.log("  Likely cause: storage-slot write to current proxy impl is wrong");
		console.log("  Fix: switch start-anvil.sh to whale impersonation.");
	} else {
		console.log(
			`✓ Account ${firstAcct} has USDC=${format(usdc, 6)}, USDT=${format(usdt, 6)}.`,
		);
		console.log("  Mints are landing on dev accounts. If the UI still shows 0,");
		console.log("  the bug is in the UI (likely connected to a different account).");
	}
}

main().catch((e) => {
	console.error(e);
	process.exit(1);
});
