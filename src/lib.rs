#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env};

const SEVEN_DAYS: u64 = 7 * 24 * 60 * 60;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    LastActiveTimestamp,
    UserBalance(Address),
    Admin,
}

#[contract]
pub struct SoroSusu;

#[contractimpl]
impl SoroSusu {
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::LastActiveTimestamp, &env.ledger().timestamp());
    }

    pub fn admin_action(env: Env) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.storage().instance().set(&DataKey::LastActiveTimestamp, &env.ledger().timestamp());
    }

    pub fn deposit(env: Env, user: Address, token_address: Address, amount: i128) {
        user.require_auth();
        let token_client = token::Client::new(&env, &token_address);
        token_client.transfer(&user, &env.current_contract_address(), &amount);
        
        let current_balance: i128 = env.storage().persistent()
            .get(&DataKey::UserBalance(user.clone()))
            .unwrap_or(0);
        env.storage().persistent().set(&DataKey::UserBalance(user), &(current_balance + amount));
    }

    pub fn emergency_withdraw(env: Env, user: Address, token_address: Address) {
        user.require_auth();
        
        let last_active: u64 = env.storage().instance()
            .get(&DataKey::LastActiveTimestamp)
            .unwrap_or(0);
        let current_time = env.ledger().timestamp();
        
        if current_time <= last_active + SEVEN_DAYS {
            panic!("Emergency withdrawal not available yet");
        }
        
        let balance: i128 = env.storage().persistent()
            .get(&DataKey::UserBalance(user.clone()))
            .unwrap_or(0);
        
        if balance > 0 {
            let token_client = token::Client::new(&env, &token_address);
            token_client.transfer(&env.current_contract_address(), &user, &balance);
            env.storage().persistent().remove(&DataKey::UserBalance(user));
        }
    }

    pub fn get_user_balance(env: Env, user: Address) -> i128 {
        env.storage().persistent()
            .get(&DataKey::UserBalance(user))
            .unwrap_or(0)
    }

    pub fn get_last_active_timestamp(env: Env) -> u64 {
        env.storage().instance()
            .get(&DataKey::LastActiveTimestamp)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::{Address as _, Ledger}, token, Address, Env};

    fn create_token_contract<'a>(env: &Env, admin: &Address) -> (token::Client<'a>, token::StellarAssetClient<'a>) {
        let contract_address = env.register_stellar_asset_contract(admin.clone());
        (
            token::Client::new(env, &contract_address),
            token::StellarAssetClient::new(env, &contract_address),
        )
    }

    #[test]
    fn test_emergency_withdraw_after_seven_days() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);

        let (token_client, token_admin) = create_token_contract(&env, &admin);
        token_admin.mint(&user, &1000);

        client.initialize(&admin);
        client.deposit(&user, &token_client.address, &500);

        assert_eq!(client.get_user_balance(&user), 500);

        env.ledger().with_mut(|li| {
            li.timestamp = li.timestamp + SEVEN_DAYS + 1;
        });

        client.emergency_withdraw(&user, &token_client.address);

        assert_eq!(client.get_user_balance(&user), 0);
        assert_eq!(token_client.balance(&user), 1000);
    }

    #[test]
    #[should_panic(expected = "Emergency withdrawal not available yet")]
    fn test_emergency_withdraw_before_seven_days() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);

        let (token_client, token_admin) = create_token_contract(&env, &admin);
        token_admin.mint(&user, &1000);

        client.initialize(&admin);
        client.deposit(&user, &token_client.address, &500);

        client.emergency_withdraw(&user, &token_client.address);
    }

    #[test]
    fn test_admin_action_updates_timestamp() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);

        client.initialize(&admin);
        let initial_timestamp = client.get_last_active_timestamp();

        env.ledger().with_mut(|li| {
            li.timestamp = li.timestamp + 100;
        });

        client.admin_action();
        let updated_timestamp = client.get_last_active_timestamp();

        assert!(updated_timestamp > initial_timestamp);
    }
}
