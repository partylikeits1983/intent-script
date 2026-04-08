//! EIP-712 typed structured data hashing.
//!
//! Implements domain separator and struct hashing that matches
//! the Solidity IntentRouter contract exactly.

use alloy_primitives::{Address, Bytes, U256, keccak256};

use std::sync::LazyLock;

/// EIP-712 domain separator type hash.
/// keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)")
static DOMAIN_TYPEHASH: LazyLock<[u8; 32]> = LazyLock::new(|| {
    keccak256(b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)")
        .0
});

/// Call type hash: keccak256("Call(address target,bytes callData,uint256 value)")
static CALL_TYPEHASH: LazyLock<[u8; 32]> =
    LazyLock::new(|| keccak256(b"Call(address target,bytes callData,uint256 value)").0);

/// IntentBatch type hash:
/// keccak256("IntentBatch(address signer,Call[] calls,address[] tokensToSweep,uint256 nonce,uint256 deadline)Call(address target,bytes callData,uint256 value)")
static INTENT_BATCH_TYPEHASH: LazyLock<[u8; 32]> = LazyLock::new(|| {
    keccak256(b"IntentBatch(address signer,Call[] calls,address[] tokensToSweep,uint256 nonce,uint256 deadline)Call(address target,bytes callData,uint256 value)").0
});

/// Compute the EIP-712 domain separator.
pub fn compute_domain_separator(
    name: &str,
    version: &str,
    chain_id: u64,
    verifying_contract: Address,
) -> [u8; 32] {
    let name_hash = keccak256(name.as_bytes());
    let version_hash = keccak256(version.as_bytes());

    // abi.encode(DOMAIN_TYPEHASH, nameHash, versionHash, chainId, verifyingContract)
    let mut encoded = Vec::with_capacity(5 * 32);
    encoded.extend_from_slice(&*DOMAIN_TYPEHASH);
    encoded.extend_from_slice(name_hash.as_slice());
    encoded.extend_from_slice(version_hash.as_slice());
    // chainId as uint256 (left-padded to 32 bytes)
    let mut chain_id_bytes = [0u8; 32];
    chain_id_bytes[24..32].copy_from_slice(&chain_id.to_be_bytes());
    encoded.extend_from_slice(&chain_id_bytes);
    // address as bytes32 (left-padded to 32 bytes)
    let mut addr_bytes = [0u8; 32];
    addr_bytes[12..32].copy_from_slice(verifying_contract.as_slice());
    encoded.extend_from_slice(&addr_bytes);

    keccak256(&encoded).0
}

/// Hash a single Call struct.
pub fn hash_call(target: Address, calldata: &Bytes, value: U256) -> [u8; 32] {
    let calldata_hash = keccak256(calldata.as_ref());

    let mut encoded = Vec::with_capacity(4 * 32);
    encoded.extend_from_slice(&*CALL_TYPEHASH);
    // address as bytes32
    let mut addr_bytes = [0u8; 32];
    addr_bytes[12..32].copy_from_slice(target.as_slice());
    encoded.extend_from_slice(&addr_bytes);
    // keccak256(callData)
    encoded.extend_from_slice(calldata_hash.as_slice());
    // value as uint256
    encoded.extend_from_slice(&value.to_be_bytes::<32>());

    keccak256(&encoded).0
}

/// Hash an array of Call structs.
pub fn hash_calls(calls: &[(Address, Bytes, U256)]) -> [u8; 32] {
    // Hash each call, then keccak256(abi.encodePacked(hashes))
    let mut packed = Vec::with_capacity(calls.len() * 32);
    for (target, calldata, value) in calls {
        let h = hash_call(*target, calldata, *value);
        packed.extend_from_slice(&h);
    }
    keccak256(&packed).0
}

/// Hash an IntentBatch struct.
pub fn hash_intent_batch(
    signer: Address,
    calls: &[(Address, Bytes, U256)],
    tokens_to_sweep: &[Address],
    nonce: u64,
    deadline: u64,
) -> [u8; 32] {
    let calls_hash = hash_calls(calls);

    // keccak256(abi.encodePacked(tokensToSweep))
    let mut tokens_packed = Vec::with_capacity(tokens_to_sweep.len() * 32);
    for token in tokens_to_sweep {
        let mut addr_bytes = [0u8; 32];
        addr_bytes[12..32].copy_from_slice(token.as_slice());
        tokens_packed.extend_from_slice(&addr_bytes);
    }
    let tokens_hash = keccak256(&tokens_packed);

    // abi.encode(INTENT_BATCH_TYPEHASH, signer, callsHash, tokensHash, nonce, deadline)
    let mut encoded = Vec::with_capacity(6 * 32);
    encoded.extend_from_slice(&*INTENT_BATCH_TYPEHASH);
    // signer as bytes32
    let mut signer_bytes = [0u8; 32];
    signer_bytes[12..32].copy_from_slice(signer.as_slice());
    encoded.extend_from_slice(&signer_bytes);
    encoded.extend_from_slice(&calls_hash);
    encoded.extend_from_slice(tokens_hash.as_slice());
    // nonce as uint256
    let mut nonce_bytes = [0u8; 32];
    nonce_bytes[24..32].copy_from_slice(&nonce.to_be_bytes());
    encoded.extend_from_slice(&nonce_bytes);
    // deadline as uint256
    let mut deadline_bytes = [0u8; 32];
    deadline_bytes[24..32].copy_from_slice(&deadline.to_be_bytes());
    encoded.extend_from_slice(&deadline_bytes);

    keccak256(&encoded).0
}

/// Compute the final EIP-712 typed data hash: keccak256("\x19\x01" || domainSeparator || structHash)
pub fn compute_typed_data_hash(domain_separator: &[u8; 32], struct_hash: &[u8; 32]) -> [u8; 32] {
    let mut encoded = Vec::with_capacity(2 + 32 + 32);
    encoded.push(0x19);
    encoded.push(0x01);
    encoded.extend_from_slice(domain_separator);
    encoded.extend_from_slice(struct_hash);
    keccak256(&encoded).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_typehash() {
        // Verify our constant matches
        let computed = keccak256(
            b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
        );
        assert_eq!(computed.0, *DOMAIN_TYPEHASH);
    }

    #[test]
    fn test_call_typehash() {
        let computed = keccak256(b"Call(address target,bytes callData,uint256 value)");
        assert_eq!(computed.0, *CALL_TYPEHASH);
    }

    #[test]
    fn test_intent_batch_typehash() {
        let computed = keccak256(b"IntentBatch(address signer,Call[] calls,address[] tokensToSweep,uint256 nonce,uint256 deadline)Call(address target,bytes callData,uint256 value)");
        assert_eq!(computed.0, *INTENT_BATCH_TYPEHASH);
    }

    #[test]
    fn test_domain_separator_computation() {
        let router_addr: Address = "0x1111111254EEB25477B68fb85Ed929f73A960582"
            .parse()
            .unwrap();
        let ds = compute_domain_separator("IntentRouter", "1", 1, router_addr);
        // Just verify it's non-zero and deterministic
        assert_ne!(ds, [0u8; 32]);
        let ds2 = compute_domain_separator("IntentRouter", "1", 1, router_addr);
        assert_eq!(ds, ds2);
    }

    #[test]
    fn test_hash_empty_calls() {
        let calls: Vec<(Address, Bytes, U256)> = vec![];
        let hash = hash_calls(&calls);
        // keccak256("") for empty encodePacked
        let expected = keccak256(&[]);
        assert_eq!(hash, expected.0);
    }

    #[test]
    fn test_typed_data_hash_structure() {
        let domain = [1u8; 32];
        let struct_hash = [2u8; 32];
        let result = compute_typed_data_hash(&domain, &struct_hash);
        // Should be keccak256("\x19\x01" || domain || structHash)
        let mut expected_input = vec![0x19, 0x01];
        expected_input.extend_from_slice(&domain);
        expected_input.extend_from_slice(&struct_hash);
        let expected = keccak256(&expected_input);
        assert_eq!(result, expected.0);
    }
}
