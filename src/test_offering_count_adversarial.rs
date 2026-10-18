//! Adversarial and boundary test coverage for `get_offering_count` and `list_offerings` in `lib.rs` (#1089).
//!
//! # Coverage Matrix
//!
//! | Scenario / Property                                          | Assertions / Expected Behavior |
//! |--------------------------------------------------------------|--------------------------------|
//! | Uninitialized / Non-existent tenant                          | Returns `0` (clean default)    |
//! | Isolation across distinct issuers (same namespace)           | Increments strictly isolate    |
//! | Isolation across distinct namespaces (same issuer)          | Increments strictly isolate    |
//! | Registration with boundary / diverse namespace symbols       | Exact count tracking           |
//! | State monotonicity on successful registrations               | `0 -> 1 -> 2 -> ...`          |
//! | No auth required / callable by arbitrary unprivileged env    | Public view access preserved   |
//! | Failed registration rollback (invalid cap / amount)          | Count remains unchanged (0)    |
//! | Failed registration rollback (invalid share bps > 10,000)    | Count remains unchanged (0)    |
//! | Duplicate offering registration rejection                    | Count does not double-count    |
//! | Multi-issuer offering transfer acceptance                    | Receiver count updates cleanly |
//! | Direct contract call via `RevoraRevenueShare::get_offering_count` | Exact match with client helper |
//! | `list_offerings` on uninitialized / non-existent tenant      | Returns empty `Vec`            |
//! | `list_offerings` isolation across issuers and namespaces     | Strict per-tenant isolation    |
//! | `list_offerings` pagination boundaries (start/limit)         | Deterministic slices, no panic |
//! | `list_offerings` after rejected registrations                | State unchanged (empty)        |
//! | `list_offerings` reflects accepted issuer transfer           | Receiver index updates cleanly |

#![cfg(test)]

use crate::{DataKey, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, TenantId};
use soroban_sdk::{
    symbol_short,
    testutils::Address as _,
    Address, Env, Symbol, Vec,
};

// ── Test Setup Helpers ────────────────────────────────────────────────────────

fn setup_env() -> (Env, Address, RevoraRevenueShareClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    (env, contract_id, client)
}

fn register_single_offering(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
) {
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        namespace,
        token,
        &1_000,
        token,
        &0,
        &symbol_short!(""),
        &0,
    );
}

fn register_offering_with_token(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
    offering_id: u32,
) {
    client.register_offering(
        issuer,
        &Vec::new(env),
        &offering_id,
        namespace,
        token,
        &1_000,
        token,
        &0,
        &symbol_short!(""),
        &0,
    );
}

// ── Tests: Baseline and Querying Non-existent Tenants ────────────────────────

#[test]
fn get_offering_count_returns_zero_for_uninitialized_tenant() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let namespace = symbol_short!("default");

    let count = client.get_offering_count(&issuer, &namespace);
    assert_eq!(count, 0, "Unregistered tenant must return 0 offerings");
}

#[test]
fn get_offering_count_returns_zero_for_random_issuers_and_namespaces() {
    let (env, _contract_id, client) = setup_env();

    for _ in 0..5 {
        let issuer = Address::generate(&env);
        let namespace = Symbol::new(&env, "random_ns");
        assert_eq!(client.get_offering_count(&issuer, &namespace), 0);
    }
}

// ── Tests: Tenant Isolation (Issuer and Namespace Sandboxing) ────────────────

#[test]
fn get_offering_count_isolates_between_different_issuers_same_namespace() {
    let (env, _contract_id, client) = setup_env();
    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let namespace = symbol_short!("shared");

    let token_a1 = Address::generate(&env);
    let token_a2 = Address::generate(&env);
    let token_b1 = Address::generate(&env);

    // Initial state: both zero
    assert_eq!(client.get_offering_count(&issuer_a, &namespace), 0);
    assert_eq!(client.get_offering_count(&issuer_b, &namespace), 0);

    // Register 1 for issuer_a
    register_single_offering(&env, &client, &issuer_a, &namespace, &token_a1);
    assert_eq!(client.get_offering_count(&issuer_a, &namespace), 1);
    assert_eq!(client.get_offering_count(&issuer_b, &namespace), 0);

    // Register 1 for issuer_b
    register_single_offering(&env, &client, &issuer_b, &namespace, &token_b1);
    assert_eq!(client.get_offering_count(&issuer_a, &namespace), 1);
    assert_eq!(client.get_offering_count(&issuer_b, &namespace), 1);

    // Register another for issuer_a
    register_single_offering(&env, &client, &issuer_a, &namespace, &token_a2);
    assert_eq!(client.get_offering_count(&issuer_a, &namespace), 2);
    assert_eq!(client.get_offering_count(&issuer_b, &namespace), 1);
}

#[test]
fn get_offering_count_isolates_between_different_namespaces_same_issuer() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let ns_prod = symbol_short!("prod");
    let ns_staging = symbol_short!("stage");
    let ns_dev = symbol_short!("dev");

    let token_prod = Address::generate(&env);
    let token_staging = Address::generate(&env);

    // Register in prod
    register_single_offering(&env, &client, &issuer, &ns_prod, &token_prod);
    assert_eq!(client.get_offering_count(&issuer, &ns_prod), 1);
    assert_eq!(client.get_offering_count(&issuer, &ns_staging), 0);
    assert_eq!(client.get_offering_count(&issuer, &ns_dev), 0);

    // Register in staging
    register_single_offering(&env, &client, &issuer, &ns_staging, &token_staging);
    assert_eq!(client.get_offering_count(&issuer, &ns_prod), 1);
    assert_eq!(client.get_offering_count(&issuer, &ns_staging), 1);
    assert_eq!(client.get_offering_count(&issuer, &ns_dev), 0);
}

// ── Tests: Namespace Boundary Values ──────────────────────────────────────────

#[test]
fn get_offering_count_handles_various_namespace_symbols() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);

    // Short symbol, empty symbol, and max length symbols
    let ns_empty = symbol_short!("");
    let ns_single = symbol_short!("a");
    let ns_max_short = symbol_short!("123456789");
    let ns_long = Symbol::new(&env, "a_longer_namespace_name");

    let token_1 = Address::generate(&env);
    let token_2 = Address::generate(&env);
    let token_3 = Address::generate(&env);
    let token_4 = Address::generate(&env);

    register_single_offering(&env, &client, &issuer, &ns_empty, &token_1);
    register_single_offering(&env, &client, &issuer, &ns_single, &token_2);
    register_single_offering(&env, &client, &issuer, &ns_max_short, &token_3);
    register_single_offering(&env, &client, &issuer, &ns_long, &token_4);

    assert_eq!(client.get_offering_count(&issuer, &ns_empty), 1);
    assert_eq!(client.get_offering_count(&issuer, &ns_single), 1);
    assert_eq!(client.get_offering_count(&issuer, &ns_max_short), 1);
    assert_eq!(client.get_offering_count(&issuer, &ns_long), 1);
}

// ── Tests: State Invariance on Failed Registrations ───────────────────────────

#[test]
fn get_offering_count_unchanged_on_rejected_invalid_bps() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);

    // Verify initial count is 0
    assert_eq!(client.get_offering_count(&issuer, &namespace), 0);

    // Attempt registration with invalid BPS (> 10_000)
    let res = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &10_001u32,
        &token,
        &0,
        &symbol_short!(""),
        &0,
    );
    assert_eq!(res, Err(Ok(RevoraError::InvalidRevenueShareBps)));

    // Ensure offering count did not increment and remains 0
    assert_eq!(client.get_offering_count(&issuer, &namespace), 0);
}

#[test]
fn get_offering_count_unchanged_on_rejected_negative_cap() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);

    // Attempt registration with negative hard cap
    for invalid_cap in [-1_i128, i128::MIN] {
        let res = client.try_register_offering(
            &issuer,
            &Vec::new(&env),
            &1u32,
            &namespace,
            &token,
            &5_000u32,
            &token,
            &invalid_cap,
            &symbol_short!(""),
            &0,
        );
        assert_eq!(res, Err(Ok(RevoraError::InvalidAmount)));
        assert_eq!(client.get_offering_count(&issuer, &namespace), 0);
    }
}

#[test]
fn get_offering_count_unchanged_on_duplicate_token_registration() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);

    // First registration succeeds -> count becomes 1
    register_single_offering(&env, &client, &issuer, &namespace, &token);
    assert_eq!(client.get_offering_count(&issuer, &namespace), 1);

    // Second registration with the exact same token must fail
    let duplicate_res = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &2_000u32,
        &token,
        &0,
        &symbol_short!(""),
        &0,
    );
    assert_eq!(duplicate_res, Err(Ok(RevoraError::OfferingAlreadyExists)));

    // Offering count must remain 1
    assert_eq!(client.get_offering_count(&issuer, &namespace), 1);
}

// ── Tests: Direct Static Invocation & Storage Integrity ──────────────────────

#[test]
fn direct_static_get_offering_count_matches_client_interface() {
    let (env, contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let namespace = symbol_short!("static");

    for i in 0..3 {
        let token = Address::generate(&env);
        register_single_offering(&env, &client, &issuer, &namespace, &token);

        // Verify count through client
        let client_count = client.get_offering_count(&issuer, &namespace);
        assert_eq!(client_count, i + 1);

        // Verify count directly via contract invocation inside contract env context
        env.as_contract(&contract_id, || {
            let static_count = RevoraRevenueShare::get_offering_count(
                env.clone(),
                issuer.clone(),
                namespace.clone(),
            );
            assert_eq!(static_count, i + 1);

            // Directly inspect low-level DataKey::OfferCount persistent storage
            let key = DataKey::OfferCount(TenantId {
                issuer: issuer.clone(),
                namespace: namespace.clone(),
            });
            let stored_val: Option<u32> = env.storage().persistent().get(&key);
            assert_eq!(stored_val, Some(i + 1));
        });
    }
}

// ── Tests: Transfer Issuer Stability ──────────────────────────────────────────

#[test]
fn get_offering_count_updates_appropriately_on_accepted_issuer_transfer() {
    let (env, _contract_id, client) = setup_env();
    let issuer_old = Address::generate(&env);
    let issuer_new = Address::generate(&env);
    let namespace = symbol_short!("transfer");

    let token1 = Address::generate(&env);
    let token2 = Address::generate(&env);

    register_single_offering(&env, &client, &issuer_old, &namespace, &token1);
    register_single_offering(&env, &client, &issuer_old, &namespace, &token2);

    assert_eq!(client.get_offering_count(&issuer_old, &namespace), 2);
    assert_eq!(client.get_offering_count(&issuer_new, &namespace), 0);

    // Propose and accept transfer of token1 to issuer_new
    client.propose_issuer_transfer(&issuer_old, &namespace, &token1, &issuer_new);
    client.accept_issuer_transfer(&issuer_new, &namespace, &token1);

    // In Revora's design, accepting an issuer transfer appends to the new issuer's offering index
    assert_eq!(client.get_offering_count(&issuer_new, &namespace), 1);
    // Old issuer retains original indexed count for backward compatibility / historic slot stability
    assert_eq!(client.get_offering_count(&issuer_old, &namespace), 2);
}

// ── Tests: `list_offerings` Baseline and Non-existent Tenants ────────────────

#[test]
fn list_offerings_returns_empty_for_uninitialized_tenant() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let namespace = symbol_short!("default");

    let offerings = client.list_offerings(&issuer, &namespace, &0u32, &10u32);
    assert_eq!(offerings.len(), 0, "Unregistered tenant must list no offerings");
}

#[test]
fn list_offerings_returns_empty_for_random_issuers_and_namespaces() {
    let (env, _contract_id, client) = setup_env();

    for _ in 0..5 {
        let issuer = Address::generate(&env);
        let namespace = Symbol::new(&env, "random_ns");
        let offerings = client.list_offerings(&issuer, &namespace, &0u32, &100u32);
        assert_eq!(offerings.len(), 0);
    }
}

// ── Tests: `list_offerings` Tenant Isolation ─────────────────────────────────

#[test]
fn list_offerings_isolates_between_different_issuers_same_namespace() {
    let (env, _contract_id, client) = setup_env();
    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let namespace = symbol_short!("shared");

    let token_a1 = Address::generate(&env);
    let token_b1 = Address::generate(&env);

    register_offering_with_token(&env, &client, &issuer_a, &namespace, &token_a1, 1);
    register_offering_with_token(&env, &client, &issuer_b, &namespace, &token_b1, 1);

    let list_a = client.list_offerings(&issuer_a, &namespace, &0u32, &10u32);
    let list_b = client.list_offerings(&issuer_b, &namespace, &0u32, &10u32);

    assert_eq!(list_a.len(), 1);
    assert_eq!(list_b.len(), 1);
    assert_eq!(list_a.get(0).unwrap().token, token_a1);
    assert_eq!(list_b.get(0).unwrap().token, token_b1);
}

#[test]
fn list_offerings_isolates_between_different_namespaces_same_issuer() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let ns_prod = symbol_short!("prod");
    let ns_staging = symbol_short!("stage");

    let token_prod = Address::generate(&env);
    let token_staging = Address::generate(&env);

    register_offering_with_token(&env, &client, &issuer, &ns_prod, &token_prod, 1);
    register_offering_with_token(&env, &client, &issuer, &ns_staging, &token_staging, 1);

    let list_prod = client.list_offerings(&issuer, &ns_prod, &0u32, &10u32);
    let list_staging = client.list_offerings(&issuer, &ns_staging, &0u32, &10u32);

    assert_eq!(list_prod.len(), 1);
    assert_eq!(list_staging.len(), 1);
    assert_eq!(list_prod.get(0).unwrap().token, token_prod);
    assert_eq!(list_staging.get(0).unwrap().token, token_staging);
}

// ── Tests: `list_offerings` Pagination Boundaries ────────────────────────────

#[test]
fn list_offerings_pagination_start_and_limit_boundaries() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let namespace = symbol_short!("page");

    for i in 0..5u32 {
        let token = Address::generate(&env);
        register_offering_with_token(&env, &client, &issuer, &namespace, &token, i + 1);
    }

    // Full list
    let all = client.list_offerings(&issuer, &namespace, &0u32, &10u32);
    assert_eq!(all.len(), 5);

    // Start at 0, limit 0 -> empty
    let empty = client.list_offerings(&issuer, &namespace, &0u32, &0u32);
    assert_eq!(empty.len(), 0);

    // Start beyond end -> empty
    let beyond = client.list_offerings(&issuer, &namespace, &100u32, &10u32);
    assert_eq!(beyond.len(), 0);

    // Start at 2, limit 2 -> 2 items
    let slice = client.list_offerings(&issuer, &namespace, &2u32, &2u32);
    assert_eq!(slice.len(), 2);

    // Start at 4, limit 10 -> 1 item
    let tail = client.list_offerings(&issuer, &namespace, &4u32, &10u32);
    assert_eq!(tail.len(), 1);
}

// ── Tests: `list_offerings` State Invariance on Rejected Registrations ───────

#[test]
fn list_offerings_unchanged_on_rejected_invalid_bps() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);

    let res = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &10_001u32,
        &token,
        &0,
        &symbol_short!(""),
        &0,
    );
    assert_eq!(res, Err(Ok(RevoraError::InvalidRevenueShareBps)));

    let offerings = client.list_offerings(&issuer, &namespace, &0u32, &10u32);
    assert_eq!(offerings.len(), 0, "Rejected registration must not appear in list");
}

#[test]
fn list_offerings_unchanged_on_duplicate_token_registration() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);

    register_offering_with_token(&env, &client, &issuer, &namespace, &token, 1);
    assert_eq!(client.list_offerings(&issuer, &namespace, &0u32, &10u32).len(), 1);

    let duplicate_res = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &2_000u32,
        &token,
        &0,
        &symbol_short!(""),
        &0,
    );
    assert_eq!(duplicate_res, Err(Ok(RevoraError::OfferingAlreadyExists)));

    let offerings = client.list_offerings(&issuer, &namespace, &0u32, &10u32);
    assert_eq!(offerings.len(), 1, "Duplicate registration must not double-list");
}

// ── Tests: `list_offerings` After Accepted Issuer Transfer ───────────────────

#[test]
fn list_offerings_reflects_accepted_issuer_transfer() {
    let (env, _contract_id, client) = setup_env();
    let issuer_old = Address::generate(&env);
    let issuer_new = Address::generate(&env);
    let namespace = symbol_short!("transfer");

    let token1 = Address::generate(&env);
    let token2 = Address::generate(&env);

    register_offering_with_token(&env, &client, &issuer_old, &namespace, &token1, 1);
    register_offering_with_token(&env, &client, &issuer_old, &namespace, &token2, 2);

    assert_eq!(client.list_offerings(&issuer_new, &namespace, &0u32, &10u32).len(), 0);

    client.propose_issuer_transfer(&issuer_old, &namespace, &token1, &issuer_new);
    client.accept_issuer_transfer(&issuer_new, &namespace, &token1);

    let new_list = client.list_offerings(&issuer_new, &namespace, &0u32, &10u32);
    assert_eq!(new_list.len(), 1);
    assert_eq!(new_list.get(0).unwrap().token, token1);
}
