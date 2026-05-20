# [WS-3B] Anvil/MetaMask chain-ID UX fixes

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/ux`, `area/local-dev`, `size/S`
**Depends on:** none

## Context

User report: "I connect MetaMask to localhost RPC and it thinks it's actually mainnet, when it's just a fork using anvil."

Root cause analysis:
- `scripts/start-anvil.sh` starts anvil with `--chain-id 31337` (good).
- `lib/wagmi-config.ts:21` — when `NEXT_PUBLIC_USE_LOCAL_FORK=true`, it routes BOTH chain ID 31337 (anvil) AND chain ID 1 (mainnet) transports to `http://127.0.0.1:8545`. So if the user has MetaMask on "Ethereum Mainnet" (chain 1) and our app is in fork mode, the app accepts it — but MetaMask's UI says "Ethereum Mainnet."
- There's no clear UI affordance to add the correct fork network to MetaMask or to verify the user is on the right chain.

## Scope

1. Add "Add intentOS Fork to MetaMask" button (visible only in local-fork mode):
   - Calls `wallet_addEthereumChain` with `{ chainId: 0x7a69, chainName: "intentOS Fork", rpcUrls: ["http://127.0.0.1:8545"], nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 } }`.
   - One click gets the user to chainId 31337 with a clearly-labeled network.
2. Remove the chain-1 → localhost aliasing in `lib/wagmi-config.ts`. In local-fork mode, only expose chain 31337. The compiler already treats anvil.json and ethereum.json as identical; no functional loss.
3. Chain-mismatch banner (`components/local-fork-banner.tsx` or new):
   - If fork mode active AND MetaMask chainId ≠ 31337, show a loud banner: "MetaMask is on Ethereum Mainnet (chain 1), but the app expects intentOS Fork (chain 31337). Click here to switch."
   - Uses wagmi's `useChainId` + `useSwitchChain`.
4. Documentation update: a short section in `docs/local-development.mdx` (lands in WS-5B) + inline-in-UI tooltip explaining that anvil is a mainnet FORK, which is why contract addresses look like mainnet, but chain ID is 31337.

## Files

- `intentOS-ui/lib/wagmi-config.ts` — remove the mainnet→localhost aliasing
- `intentOS-ui/components/local-fork-banner.tsx` — upgrade with chain-mismatch detection
- `intentOS-ui/components/add-fork-to-metamask-button.tsx` (new)
- `intentOS-ui/components/connect-button.tsx` — integrate the mismatch banner

## Acceptance criteria

- [ ] With `NEXT_PUBLIC_USE_LOCAL_FORK=true` and a fresh MetaMask, clicking "Add Fork" adds chain 31337 named "intentOS Fork."
- [ ] When connected to chain 1 in fork mode, the banner appears and offers a one-click switch.
- [ ] After the switch, MetaMask shows "intentOS Fork (31337)" — not "Ethereum Mainnet."
- [ ] `lib/wagmi-config.ts` no longer routes chain 1 RPC to localhost.
- [ ] Clear tooltip or docs link explaining the fork vs mainnet distinction.
