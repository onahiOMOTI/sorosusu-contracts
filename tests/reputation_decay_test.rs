#![cfg(test)]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token,
    testutils::{Address as _, AuthorizedFunction, Ledger as_},
    Address, Env, String, Symbol, Vec,
};
use std::time::{SystemTime, UNIX_EPOCH};

use soro_susu_contracts::{SoroSusuContract, SoroSusuContractClient};

#[contract]
pub struct TestContract;

#[contractimpl]
impl TestContract {
    pub fn init(env: Env) {
        // Initialize the SoroSusu contract
        let admin = Address::generate(&env);
        SoroSusuContractClient::new(&env, &env.register_contract(None, SoroSusuContract {}))
            .init(&admin);
    }
}

fn create_test_env() -> (Env, Address, Address) {
    let env = Env::default();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    // Initialize contract
    SoroSusuContractClient::new(&env, &env.register_contract(None, SoroSusuContract {}))
        .init(&admin);
    
    (env, admin, user)
}

fn setup_circle(env: &Env, admin: &Address) -> u64 {
    let token = Address::generate(env);
    let nft_contract = Address::generate(env);
    
    SoroSusuContractClient::new(env, &env.register_contract(None, SoroSusuContract {}))
        .create_circle(
            admin,
            100_000_0, // 100 XLM contribution
            5,          // 5 members
            token,
            2592000,    // 30 days cycle
            200,        // 2% insurance fee
            nft_contract,
        )
}

#[test]
fn test_reputation_decay_basic() {
    let (env, admin, user) = create_test_env();
    let circle_id = setup_circle(&env, &admin);
    
    // Set initial trust score to 100
    let initial_timestamp = env.ledger().timestamp();
    
    // Create social capital entry manually for testing
    let social_capital_key = soro_susu_contracts::DataKey::SocialCapital(user.clone(), circle_id);
    let initial_social_capital = soro_susu_contracts::SocialCapital {
        member: user.clone(),
        circle_id,
        leniency_given: 0,
        leniency_received: 0,
        voting_participation: 0,
        trust_score: 100,
        last_activity_timestamp: initial_timestamp,
        decay_count: 0,
    };
    
    env.storage().instance().set(&social_capital_key, &initial_social_capital);
    
    // Fast forward time by 19 months (past 18 month threshold)
    let months_19_later = initial_timestamp + (19 * 2592000); // 19 months
    env.ledger().set_timestamp(months_19_later);
    
    // Apply decay
    SoroSusuContractClient::new(&env, &env.register_contract(None, SoroSusuContract {}))
        .apply_reputation_decay(&user, &circle_id);
    
    // Check that decay was applied
    let updated_social_capital: soro_susu_contracts::SocialCapital = 
        env.storage().instance().get(&social_capital_key).unwrap();
    
    // Should have decayed for 1 month (19 - 18 = 1)
    // 5% decay on 100 = 5, so new score should be 95
    assert_eq!(updated_social_capital.trust_score, 95);
    assert_eq!(updated_social_capital.decay_count, 1);
}

#[test]
fn test_reputation_decay_multiple_months() {
    let (env, admin, user) = create_test_env();
    let circle_id = setup_circle(&env, &admin);
    
    // Set initial trust score to 100
    let initial_timestamp = env.ledger().timestamp();
    
    let social_capital_key = soro_susu_contracts::DataKey::SocialCapital(user.clone(), circle_id);
    let initial_social_capital = soro_susu_contracts::SocialCapital {
        member: user.clone(),
        circle_id,
        leniency_given: 0,
        leniency_received: 0,
        voting_participation: 0,
        trust_score: 100,
        last_activity_timestamp: initial_timestamp,
        decay_count: 0,
    };
    
    env.storage().instance().set(&social_capital_key, &initial_social_capital);
    
    // Fast forward time by 24 months (6 months past threshold)
    let months_24_later = initial_timestamp + (24 * 2592000); // 24 months
    env.ledger().set_timestamp(months_24_later);
    
    // Apply decay
    SoroSusuContractClient::new(&env, &env.register_contract(None, SoroSusuContract {}))
        .apply_reputation_decay(&user, &circle_id);
    
    // Check that decay was applied
    let updated_social_capital: soro_susu_contracts::SocialCapital = 
        env.storage().instance().get(&social_capital_key).unwrap();
    
    // Should have decayed for 6 months (24 - 18 = 6)
    // 6 months * 5% = 30% total decay on 100 = 30, so new score should be 70
    assert_eq!(updated_social_capital.trust_score, 70);
    assert_eq!(updated_social_capital.decay_count, 6);
}

#[test]
fn test_reputation_decay_floor_at_zero() {
    let (env, admin, user) = create_test_env();
    let circle_id = setup_circle(&env, &admin);
    
    // Set initial trust score to 20 (low score)
    let initial_timestamp = env.ledger().timestamp();
    
    let social_capital_key = soro_susu_contracts::DataKey::SocialCapital(user.clone(), circle_id);
    let initial_social_capital = soro_susu_contracts::SocialCapital {
        member: user.clone(),
        circle_id,
        leniency_given: 0,
        leniency_received: 0,
        voting_participation: 0,
        trust_score: 20,
        last_activity_timestamp: initial_timestamp,
        decay_count: 0,
    };
    
    env.storage().instance().set(&social_capital_key, &initial_social_capital);
    
    // Fast forward time by 38 months (20 months past threshold)
    let months_38_later = initial_timestamp + (38 * 2592000); // 38 months
    env.ledger().set_timestamp(months_38_later);
    
    // Apply decay
    SoroSusuContractClient::new(&env, &env.register_contract(None, SoroSusuContract {}))
        .apply_reputation_decay(&user, &circle_id);
    
    // Check that decay was applied but floor at 0
    let updated_social_capital: soro_susu_contracts::SocialCapital = 
        env.storage().instance().get(&social_capital_key).unwrap();
    
    // Should decay to 0 (can't go negative)
    assert_eq!(updated_social_capital.trust_score, 0);
    assert_eq!(updated_social_capital.decay_count, 20);
}

#[test]
fn test_no_decay_before_threshold() {
    let (env, admin, user) = create_test_env();
    let circle_id = setup_circle(&env, &admin);
    
    // Set initial trust score to 100
    let initial_timestamp = env.ledger().timestamp();
    
    let social_capital_key = soro_susu_contracts::DataKey::SocialCapital(user.clone(), circle_id);
    let initial_social_capital = soro_susu_contracts::SocialCapital {
        member: user.clone(),
        circle_id,
        leniency_given: 0,
        leniency_received: 0,
        voting_participation: 0,
        trust_score: 100,
        last_activity_timestamp: initial_timestamp,
        decay_count: 0,
    };
    
    env.storage().instance().set(&social_capital_key, &initial_social_capital);
    
    // Fast forward time by 17 months (before 18 month threshold)
    let months_17_later = initial_timestamp + (17 * 2592000); // 17 months
    env.ledger().set_timestamp(months_17_later);
    
    // Apply decay
    SoroSusuContractClient::new(&env, &env.register_contract(None, SoroSusuContract {}))
        .apply_reputation_decay(&user, &circle_id);
    
    // Check that no decay was applied
    let updated_social_capital: soro_susu_contracts::SocialCapital = 
        env.storage().instance().get(&social_capital_key).unwrap();
    
    // Should have no decay
    assert_eq!(updated_social_capital.trust_score, 100);
    assert_eq!(updated_social_capital.decay_count, 0);
}

#[test]
fn test_activity_timestamp_update() {
    let (env, admin, user) = create_test_env();
    let circle_id = setup_circle(&env, &admin);
    
    // Join circle to trigger activity update
    SoroSusuContractClient::new(&env, &env.register_contract(None, SoroSusuContract {}))
        .join_circle(&user, &circle_id, 1, None);
    
    // Check that activity timestamp was set
    let activity_key = soro_susu_contracts::DataKey::LastActivityTimestamp(user.clone());
    let activity_timestamp: u64 = env.storage().instance().get(&activity_key).unwrap();
    
    assert!(activity_timestamp > 0);
    
    // Check social capital was created with activity timestamp
    let social_capital_key = soro_susu_contracts::DataKey::SocialCapital(user, circle_id);
    let social_capital: soro_susu_contracts::SocialCapital = 
        env.storage().instance().get(&social_capital_key).unwrap();
    
    assert!(social_capital.last_activity_timestamp > 0);
}

#[test]
fn test_get_reputation_with_decay() {
    let (env, admin, user) = create_test_env();
    let circle_id = setup_circle(&env, &admin);
    
    // Set initial trust score to 100
    let initial_timestamp = env.ledger().timestamp();
    
    let social_capital_key = soro_susu_contracts::DataKey::SocialCapital(user.clone(), circle_id);
    let initial_social_capital = soro_susu_contracts::SocialCapital {
        member: user.clone(),
        circle_id,
        leniency_given: 0,
        leniency_received: 0,
        voting_participation: 0,
        trust_score: 100,
        last_activity_timestamp: initial_timestamp,
        decay_count: 0,
    };
    
    env.storage().instance().set(&social_capital_key, &initial_social_capital);
    
    // Fast forward time by 20 months (2 months past threshold)
    let months_20_later = initial_timestamp + (20 * 2592000); // 20 months
    env.ledger().set_timestamp(months_20_later);
    
    // Get reputation with decay
    let reputation = SoroSusuContractClient::new(&env, &env.register_contract(None, SoroSusuContract {}))
        .get_reputation_with_decay(&user, &circle_id);
    
    // Should have decayed for 2 months (20 - 18 = 2)
    // 2 months * 5% = 10% total decay on 100 = 10, so new score should be 90
    assert_eq!(reputation.trust_score, 90);
    assert_eq!(reputation.decay_count, 2);
}

#[test]
fn test_deposit_updates_activity() {
    let (env, admin, user) = create_test_env();
    let circle_id = setup_circle(&env, &admin);
    
    // Join circle first
    SoroSusuContractClient::new(&env, &env.register_contract(None, SoroSusuContract {}))
        .join_circle(&user, &circle_id, 1, None);
    
    let initial_activity_time = env.ledger().timestamp();
    
    // Fast forward time and make deposit
    env.ledger().set_timestamp(initial_activity_time + 86400); // 1 day later
    
    // Mock token transfer for deposit
    let token_client = token::StellarAssetClient::new(&env, &Address::generate(&env));
    token_client.mint(&user, &100_000_0);
    
    SoroSusuContractClient::new(&env, &env.register_contract(None, SoroSusuContract {}))
        .deposit(&user, &circle_id);
    
    // Check that activity timestamp was updated
    let social_capital_key = soro_susu_contracts::DataKey::SocialCapital(user, circle_id);
    let social_capital: soro_susu_contracts::SocialCapital = 
        env.storage().instance().get(&social_capital_key).unwrap();
    
    assert_eq!(social_capital.last_activity_timestamp, initial_activity_time + 86400);
}
