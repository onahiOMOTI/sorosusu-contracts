#![no_std]
#![cfg_attr(test, allow(dead_code))]

use soroban_sdk::{
    contract, contracterror, contractimpl, contractmeta, contracttype, panic_with_error,
    symbol_short, token, Address, Env, Map, Symbol, Vec,
};

const SEVEN_DAYS: u64 = 7 * 24 * 60 * 60;
const MAX_MEMBERS: u32 = 50;

const FEE_BASIS_POINTS_KEY: Symbol = symbol_short!("fee_bps");
const TREASURY_KEY: Symbol = symbol_short!("treasury");
const ADMIN_KEY: Symbol = symbol_short!("admin");
const MEMBERS_KEY: Symbol = symbol_short!("members");
const CONTRIBS_KEY: Symbol = symbol_short!("contribs");
const IS_PUBLIC_KEY: Symbol = symbol_short!("is_pub");
const INVITE_CODE_KEY: Symbol = symbol_short!("inv_code");
const MEMBERS_COUNT_KEY: Symbol = symbol_short!("m_count");
const MAX_BASIS_POINTS: u32 = 10_000;

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    LastActiveTimestamp,
    UserBalance(Address),
    Admin,
    Circle(u32),
    CircleCount,
}

#[derive(Clone)]
#[contracttype]
pub struct Circle {
    admin: Address,
    contribution: i128,
    members: Vec<Address>,
    is_random_queue: bool,
    payout_queue: Vec<Address>,
    has_received_payout: Vec<bool>,
    current_payout_index: u32,
    total_volume_distributed: i128,
    cycle_number: u32,
}

#[derive(Clone)]
#[contracttype]
pub struct CycleCompletedEvent {
    group_id: u32,
    total_volume_distributed: i128,
}

#[derive(Clone)]
#[contracttype]
pub struct GroupRolloverEvent {
    group_id: u32,
    new_cycle_number: u32,
}

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    CycleNotComplete = 1001,
    InsufficientAllowance = 1002,
    AlreadyJoined = 1003,
    CircleNotFound = 1004,
    Unauthorized = 1005,
    MaxMembersReached = 1006,
    CircleNotFinalized = 1007,
    InvalidFeeConfig = 1008,
    MemberNotFound = 1009,
    PenaltyExceedsContribution = 1010,
    InvalidInvite = 1011,
    MemberLimitExceeded = 1012,
}

#[contract]
pub struct SoroSusu;

fn read_circle(env: &Env, id: u32) -> Circle {
    let key = DataKey::Circle(id);
    let storage = env.storage().instance();
    match storage.get(&key) {
        Some(circle) => circle,
        None => panic_with_error!(env, Error::CircleNotFound),
    }
}

fn write_circle(env: &Env, id: u32, circle: &Circle) {
    let key = DataKey::Circle(id);
    let storage = env.storage().instance();
    storage.set(&key, circle);
}

fn next_circle_id(env: &Env) -> u32 {
    let key = DataKey::CircleCount;
    let storage = env.storage().instance();
    let current: u32 = storage.get(&key).unwrap_or(0);
    let next = current.saturating_add(1);
    storage.set(&key, &next);
    next
}

#[contractimpl]
impl SoroSusu {
    // ------------------------------------------------------------------------
    // Legacy methods from HEAD
    // ------------------------------------------------------------------------
    pub fn init_legacy(env: Env, admin: Address) {
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

    // ------------------------------------------------------------------------
    // Circle methods from UPSTREAM
    // ------------------------------------------------------------------------
    pub fn create_circle(env: Env, admin: Address, contribution: i128, is_random_queue: bool) -> u32 {
        admin.require_auth();
        let id = next_circle_id(&env);
        let members = Vec::new(&env);
        let payout_queue = Vec::new(&env);
        let has_received_payout = Vec::new(&env);
        let circle = Circle {
            admin,
            contribution,
            members,
            is_random_queue,
            payout_queue,
            has_received_payout,
            current_payout_index: 0,
            total_volume_distributed: 0,
            cycle_number: 1,
        };
        write_circle(&env, id, &circle);
        id
    }

    pub fn join_circle(env: Env, invoker: Address, circle_id: u32) {
        invoker.require_auth();
        let mut circle = read_circle(&env, circle_id);
        for member in circle.members.iter() {
            if member == invoker {
                panic_with_error!(&env, Error::AlreadyJoined);
            }
        }
        let member_count: u32 = circle.members.len();
        if member_count >= MAX_MEMBERS {
            panic_with_error!(&env, Error::MaxMembersReached);
        }
        circle.members.push_back(invoker);
        circle.has_received_payout.push_back(false);
        write_circle(&env, circle_id, &circle);
    }

    pub fn process_payout(env: Env, circle_id: u32, recipient: Address) {
        let mut circle = read_circle(&env, circle_id);

        circle.admin.require_auth();

        let mut member_index = None;
        for (i, member) in circle.members.iter().enumerate() {
            if member == recipient {
                member_index = Some(i as u32);
                break;
            }
        }

        if member_index.is_none() {
            panic_with_error!(&env, Error::Unauthorized);
        }

        let index = member_index.unwrap();

        if circle.has_received_payout.get(index).unwrap_or(false) == true {
            panic_with_error!(&env, Error::Unauthorized);
        }

        circle.has_received_payout.set(index, true);
        circle.current_payout_index += 1;

        circle.total_volume_distributed += circle.contribution;

        let all_paid = circle.has_received_payout.iter().all(|paid| paid);

        if all_paid {
            let event = CycleCompletedEvent {
                group_id: circle_id,
                total_volume_distributed: circle.total_volume_distributed,
            };
            env.events().publish(
                (symbol_short!("CYCLE_CMP"),),
                event,
            );
        }

        write_circle(&env, circle_id, &circle);
    }

    pub fn rollover_group(env: Env, circle_id: u32) {
        let mut circle = read_circle(&env, circle_id);

        circle.admin.require_auth();

        for received in circle.has_received_payout.iter() {
            if !received {
                panic_with_error!(&env, Error::CycleNotComplete);
            }
        }

        circle.cycle_number += 1;
        circle.current_payout_index = 0;

        for i in 0..circle.has_received_payout.len() {
            circle.has_received_payout.set(i, false);
        }

        circle.total_volume_distributed = 0;

        let event = GroupRolloverEvent {
            group_id: circle_id,
            new_cycle_number: circle.cycle_number,
        };
        env.events().publish(
            (symbol_short!("GRP_ROLL"),),
            event,
        );

        write_circle(&env, circle_id, &circle);
    }

    pub fn finalize_circle(env: Env, circle_id: u32) {
        let mut circle = read_circle(&env, circle_id);

        circle.admin.require_auth();

        if !circle.payout_queue.is_empty() {
            return;
        }

        if circle.is_random_queue {
            let mut shuffled_members = circle.members.clone();
            env.prng().shuffle(&mut shuffled_members);
            circle.payout_queue = shuffled_members;
        } else {
            circle.payout_queue = circle.members.clone();
        }

        write_circle(&env, circle_id, &circle);
    }

    pub fn get_payout_queue(env: Env, circle_id: u32) -> Vec<Address> {
        let circle = read_circle(&env, circle_id);
        circle.payout_queue
    }

    pub fn get_cycle_info(env: Env, circle_id: u32) -> (u32, u32, i128) {
        let circle = read_circle(&env, circle_id);
        (
            circle.cycle_number,
            circle.current_payout_index,
            circle.total_volume_distributed,
        )
    }

    pub fn get_payout_status(env: Env, circle_id: u32) -> Vec<bool> {
        let circle = read_circle(&env, circle_id);
        circle.has_received_payout
    }
}

contractmeta!(
    key = "Description",
    val = "SoroSusu ROSCA protocol with protocol payout fee"
);

#[contract]
pub struct SorosusuContracts;

#[contractimpl]
impl SorosusuContracts {
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&ADMIN_KEY) {
            return Err(Error::Unauthorized);
        }
        env.storage().instance().set(&ADMIN_KEY, &admin);
        env.storage().instance().set(&FEE_BASIS_POINTS_KEY, &0u32);
        env.storage().instance().set(&IS_PUBLIC_KEY, &true);
        
        let empty_members: Vec<Address> = Vec::new(&env);
        let empty_contribs: Map<Address, i128> = Map::new(&env);
        env.storage().instance().set(&MEMBERS_KEY, &empty_members);
        env.storage().instance().set(&CONTRIBS_KEY, &empty_contribs);
        
        Ok(())
    }

    pub fn set_protocol_fee(
        env: Env,
        fee_basis_points: u32,
        treasury: Address,
    ) -> Result<(), Error> {
        Self::require_admin(&env)?;
        if fee_basis_points > MAX_BASIS_POINTS {
            return Err(Error::InvalidFeeConfig);
        }
        env.storage().instance().set(&FEE_BASIS_POINTS_KEY, &fee_basis_points);
        env.storage().instance().set(&TREASURY_KEY, &treasury);
        Ok(())
    }

    pub fn fee_basis_points(env: Env) -> u32 {
        env.storage()
            .instance()
            .get::<_, u32>(&FEE_BASIS_POINTS_KEY)
            .unwrap_or(0)
    }

    pub fn treasury_address(env: Env) -> Option<Address> {
        env.storage().instance().get::<_, Address>(&TREASURY_KEY)
    }

    pub fn compute_and_transfer_payout(
        env: Env,
        token: Address,
        from: Address,
        recipient: Address,
        gross_payout: i128,
    ) -> Result<(), Error> {
        let fee_bps = env.storage().instance().get::<_, u32>(&FEE_BASIS_POINTS_KEY).unwrap_or(0);
        let fee = if fee_bps == 0 {
            0_i128
        } else {
            (gross_payout as i128 * fee_bps as i128) / MAX_BASIS_POINTS as i128
        };
        let net_payout = gross_payout - fee;

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&from, &recipient, &net_payout);

        if fee > 0 {
            let treasury: Address = env
                .storage()
                .instance()
                .get::<_, Address>(&TREASURY_KEY)
                .ok_or(Error::InvalidFeeConfig)?;
            token_client.transfer(&from, &treasury, &fee);
        }

        Ok(())
    }

    pub fn kick_member(
        env: Env,
        token: Address,
        member: Address,
        penalty: i128,
    ) -> Result<(), Error> {
        Self::require_admin(&env)?;

        let mut members: Vec<Address> = env.storage().instance().get(&MEMBERS_KEY).unwrap_or(Vec::new(&env));
        let mut member_index = None;
        
        for (i, m) in members.iter().enumerate() {
            if m == member {
                member_index = Some(i as u32);
                break;
            }
        }

        let index = member_index.ok_or(Error::MemberNotFound)?;

        let mut contribs: Map<Address, i128> = env.storage().instance().get(&CONTRIBS_KEY).unwrap_or(Map::new(&env));
        let total_contributed = contribs.get(member.clone()).unwrap_or(0);

        if total_contributed < penalty {
            return Err(Error::PenaltyExceedsContribution);
        }

        members.remove(index);
        contribs.remove(member.clone());
        
        env.storage().instance().set(&MEMBERS_KEY, &members);
        env.storage().instance().set(&CONTRIBS_KEY, &contribs);

        let refund = total_contributed - penalty;
        let contract_address = env.current_contract_address();
        let token_client = token::Client::new(&env, &token);

        if refund > 0 {
            token_client.transfer(&contract_address, &member, &refund);
        }

        if penalty > 0 {
            if let Some(treasury) = Self::treasury_address(env.clone()) {
                token_client.transfer(&contract_address, &treasury, &penalty);
            }
        }

        env.events().publish(
            (symbol_short!("Kicked"), member.clone()),
            (refund, penalty),
        );

        Ok(())
    }

    pub fn join(env: Env, member: Address, invite_code: Option<u64>) -> Result<(), Error> {
        member.require_auth();

        let mut count: u32 = env.storage().instance().get(&MEMBERS_COUNT_KEY).unwrap_or(0);
        if count >= MAX_MEMBERS {
            return Err(Error::MemberLimitExceeded);
        }

        let mut members: Vec<Address> = env.storage().instance().get(&MEMBERS_KEY).unwrap_or(Vec::new(&env));
        if members.contains(&member) {
            return Err(Error::AlreadyJoined);
        }

        let is_public: bool = env.storage().instance().get(&IS_PUBLIC_KEY).unwrap_or(true);

        if !is_public {
            let mut authorized_by_code = false;
            if let Some(code) = invite_code {
                if let Some(expected_code) = env.storage().instance().get::<_, u64>(&INVITE_CODE_KEY) {
                    if code == expected_code {
                        authorized_by_code = true;
                    }
                }
            }
            if !authorized_by_code {
                Self::require_admin(&env)?;
            }
        }

        members.push_back(member.clone());
        env.storage().instance().set(&MEMBERS_KEY, &members);

        count += 1;
        env.storage().instance().set(&MEMBERS_COUNT_KEY, &count);

        let mut contribs: Map<Address, i128> = env.storage().instance().get(&CONTRIBS_KEY).unwrap_or(Map::new(&env));
        contribs.set(member, 0);
        env.storage().instance().set(&CONTRIBS_KEY, &contribs);

        Ok(())
    }

    pub fn member_count(env: Env) -> u32 {
        env.storage().instance().get(&MEMBERS_COUNT_KEY).unwrap_or(0)
    }

    fn require_admin(env: &Env) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&ADMIN_KEY)
            .ok_or(Error::Unauthorized)?;
        admin.require_auth();
        Ok(())
    }
    
    pub fn set_privacy_config(
        env: Env,
        is_public: bool,
        invite_code: Option<u64>,
    ) -> Result<(), Error> {
        Self::require_admin(&env)?;
        env.storage().instance().set(&IS_PUBLIC_KEY, &is_public);
        if let Some(code) = invite_code {
            env.storage().instance().set(&INVITE_CODE_KEY, &code);
        } else {
            env.storage().instance().remove(&INVITE_CODE_KEY);
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    extern crate std;
    use super::*;
    use soroban_sdk::{testutils::{Address as _, Ledger, Events}, token, Address, Env, IntoVal};

    // --- Legacy Tests (HEAD) ---
    fn create_token_contract<'a>(env: &Env, admin: &Address) -> (token::Client<'a>, token::StellarAssetClient<'a>) {
        let contract_address = env.register_stellar_asset_contract_v2(admin.clone());
        (
            token::Client::new(env, &contract_address.address()),
            token::StellarAssetClient::new(env, &contract_address.address()),
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

        client.init_legacy(&admin);
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

        client.init_legacy(&admin);
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

        client.init_legacy(&admin);
        let initial_timestamp = client.get_last_active_timestamp();

        env.ledger().with_mut(|li| {
            li.timestamp = li.timestamp + 100;
        });

        client.admin_action();
        let updated_timestamp = client.get_last_active_timestamp();

        assert!(updated_timestamp > initial_timestamp);
    }

    fn setup(env: &Env) -> (SorosusuContractsClient, Address) {
        let contract_id = env.register_contract(None, SorosusuContracts);
        let admin = Address::generate(env);
        let client = SorosusuContractsClient::new(env, &contract_id);
        (client, admin)
    }

    #[test]
    fn set_protocol_fee_rejects_over_max() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        client.initialize(&admin);
        let treasury = Address::generate(&env);
        env.mock_all_auths();
        let result = client.try_set_protocol_fee(&10_001, &treasury);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().unwrap(), Error::InvalidFeeConfig);
    }

    #[test]
    fn fee_basis_points_and_treasury_getters() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        client.initialize(&admin);
        assert_eq!(client.fee_basis_points(), 0);
        assert!(client.treasury_address().is_none());

        let treasury = Address::generate(&env);
        env.mock_all_auths();
        client.set_protocol_fee(&50, &treasury);
        assert_eq!(client.fee_basis_points(), 50);
        assert_eq!(client.treasury_address(), Some(treasury));
    }

    #[test]
    fn kick_member_fails_if_not_found() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        client.initialize(&admin);
        
        let dummy_token = Address::generate(&env);
        let dummy_member = Address::generate(&env);
        
        env.mock_all_auths();
        let result = client.try_kick_member(&dummy_token, &dummy_member, &0);
        
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().unwrap(), Error::MemberNotFound);
    }

    #[test]
    fn test_join_public_circle() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        client.initialize(&admin);

        let member = Address::generate(&env);
        env.mock_all_auths();

        client.join(&member, &None);

        let res = client.try_join(&member, &None);
        assert_eq!(res.unwrap_err().unwrap(), Error::AlreadyJoined);
    }

    #[test]
    fn test_join_private_circle_with_invite() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        env.mock_all_auths();
        client.initialize(&admin);

        client.set_privacy_config(&false, &Some(12345));

        let member = Address::generate(&env);
        client.join(&member, &Some(12345));
    }

    #[test]
    fn test_join_private_circle_with_admin_auth() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        env.mock_all_auths(); 
        client.initialize(&admin);
        client.set_privacy_config(&false, &Some(12345));

        let member = Address::generate(&env);
        client.join(&member, &None);
    }

    #[test]
    fn test_member_limit_boundary() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        client.initialize(&admin);

        env.mock_all_auths();

        for _ in 0..50 {
            let user = Address::generate(&env);
            client.join(&user, &None);
        }

        assert_eq!(client.member_count(), 50);

        let user_51 = Address::generate(&env);
        let result = client.try_join(&user_51, &None);
        
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().unwrap(), Error::MemberLimitExceeded);
    }

    // --- Upstream Tests ---
    #[test]
    fn join_circle_enforces_max_members() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);
        let contribution = 10_i128;
        let circle_id = client.create_circle(&admin, &contribution, &false);

        for _ in 0..MAX_MEMBERS {
            let member = Address::generate(&env);
            env.mock_all_auths();
            client.join_circle(&member, &circle_id);
        }

        let extra_member = Address::generate(&env);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.join_circle(&extra_member, &circle_id);
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_random_queue_finalization() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);
        let contribution = 10_i128;

        let circle_id = client.create_circle(&admin, &contribution, &true);

        let mut _members = std::vec::Vec::new();
        for _ in 0..5 { _members.push(Address::generate(&env)); }
        let members = Vec::from_slice(&env, &_members);

        for member in members.iter() {
            env.mock_all_auths();
            client.join_circle(&member, &circle_id);
        }

        client.finalize_circle(&circle_id);

        let payout_queue = client.get_payout_queue(&circle_id);

        assert_eq!(payout_queue.len(), 5);

        for member in members.iter() {
            assert!(payout_queue.contains(&member));
        }
    }

    #[test]
    fn test_process_payout_and_cycle_completion() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);
        let contribution = 100_i128;

        let circle_id = client.create_circle(&admin, &contribution, &false);
        let mut _members = std::vec::Vec::new();
        for _ in 0..3 { _members.push(Address::generate(&env)); }
        let members = Vec::from_slice(&env, &_members);

        for member in members.iter() {
            env.mock_all_auths();
            client.join_circle(&member, &circle_id);
        }

        client.finalize_circle(&circle_id);

        for member in members.iter() {
            client.process_payout(&circle_id, &member);
        }

        let (cycle_num, payout_index, total_volume) = client.get_cycle_info(&circle_id);
        assert_eq!(cycle_num, 1);
        assert_eq!(payout_index, 3);
        assert_eq!(total_volume, 300_i128);

        let events = env.events().all();
        assert_eq!(events.len(), 1);

        let event = events.get(0).unwrap();
        let topic0: soroban_sdk::Symbol = event.1.get(0).unwrap().into_val(&env);
        assert_eq!(topic0, symbol_short!("CYCLE_CMP"));
    }

    #[test]
    fn test_sequential_queue_finalization() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);
        let contribution = 10_i128;

        let circle_id = client.create_circle(&admin, &contribution, &false);

        let mut _members = std::vec::Vec::new();
        for _ in 0..5 { _members.push(Address::generate(&env)); }
        let members = Vec::from_slice(&env, &_members);

        for member in members.iter() {
            env.mock_all_auths();
            client.join_circle(&member, &circle_id);
        }
        
        client.finalize_circle(&circle_id);

        let payout_queue = client.get_payout_queue(&circle_id);
        for (i, member) in members.iter().enumerate() {
            assert_eq!(payout_queue.get(i as u32), Some(member.clone()));
        }
    }

    #[test]
    fn test_group_rollover() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);
        let contribution = 50_i128;

        let circle_id = client.create_circle(&admin, &contribution, &false);
        let mut _members = std::vec::Vec::new();
        for _ in 0..2 { _members.push(Address::generate(&env)); }
        let members = Vec::from_slice(&env, &_members);

        for member in members.iter() {
            env.mock_all_auths();
            client.join_circle(&member, &circle_id);
        }

        client.finalize_circle(&circle_id);

        for member in members.iter() {
            client.process_payout(&circle_id, &member);
        }

        client.rollover_group(&circle_id);

        let (cycle_num, payout_index, total_volume) = client.get_cycle_info(&circle_id);
        assert_eq!(cycle_num, 2);
        assert_eq!(payout_index, 0);
        assert_eq!(total_volume, 0_i128);
    }

    #[test]
    fn test_payout_unauthorized() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);
        let contribution = 10_i128;

        let circle_id = client.create_circle(&admin, &contribution, &true);

        let member = Address::generate(&env);
        env.mock_all_auths();
        client.join_circle(&member, &circle_id);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.finalize_circle(&circle_id);
            client.process_payout(&circle_id, &member);
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_payout() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);
        let contribution = 10_i128;

        let circle_id = client.create_circle(&admin, &contribution, &false);
        let member = Address::generate(&env);
        env.mock_all_auths();
        client.join_circle(&member, &circle_id);

        client.finalize_circle(&circle_id);
        client.process_payout(&circle_id, &member);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.process_payout(&circle_id, &member);
        }));
        assert!(result.is_err());
    }
}
