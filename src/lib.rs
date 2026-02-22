#![cfg_attr(test, allow(dead_code))]
#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contractmeta, contracttype, panic_with_error,
    symbol_short, token, Address, Env, Map, Symbol, Vec,
};

const FEE_BASIS_POINTS_KEY: Symbol = symbol_short!("fee_bps");
const TREASURY_KEY: Symbol = symbol_short!("treasury");
const ADMIN_KEY: Symbol = symbol_short!("admin");
const MEMBERS_KEY: Symbol = symbol_short!("members");
const CONTRIBS_KEY: Symbol = symbol_short!("contribs");
const USER_BAL_KEY: Symbol = symbol_short!("usr_bal");
const LAST_ACTIVE_KEY: Symbol = symbol_short!("last_act");
const MAX_BASIS_POINTS: u32 = 10_000;
const EMERGENCY_WITHDRAWAL_DELAY_SECS: u64 = 7 * 24 * 60 * 60;
const MAX_MEMBERS: u32 = 50;

contractmeta!(
    key = "Description",
    val = "SoroSusu ROSCA protocol with protocol payout fee"
);

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Circle(u32),
    CircleCount,
}

#[derive(Clone)]
#[contracttype]
pub struct Circle {
    pub admin: Address,
    pub contribution: i128,
    pub members: Vec<Address>,
    pub is_random_queue: bool,
    pub payout_queue: Vec<Address>,
    pub has_received_payout: Vec<bool>,
    pub cycle_number: u32,
    pub current_payout_index: u32,
    pub total_volume_distributed: i128,
    pub is_dissolved: bool,
    pub dissolution_votes: Vec<Address>,
    pub contributions_paid: Vec<i128>,
}

#[derive(Clone)]
#[contracttype]
pub struct CycleCompletedEvent {
    pub group_id: u32,
    pub total_volume_distributed: i128,
}

#[derive(Clone)]
#[contracttype]
pub struct GroupRolloverEvent {
    pub group_id: u32,
    pub new_cycle_number: u32,
}

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    CircleNotFound = 1001,
    Unauthorized = 1002,
    AlreadyJoined = 1003,
    MaxMembersReached = 1004,
    AlreadyVoted = 1005,
    NotMember = 1006,
    AlreadyDissolved = 1007,
    NotDissolved = 1008,
    InvalidFeeConfig = 1009,
    PenaltyExceedsContribution = 1010,
    MemberAlreadyExists = 1011,
    EmergencyWithdrawalNotAvailable = 1012,
    CircleNotFinalized = 1013,
    CycleNotComplete = 1014,
    PayoutAlreadyReceived = 1015,
    InvalidCircleState = 1016,
    MemberNotFound = 1017,
}

#[contract]
pub struct SoroSusu;

fn read_circle(env: &Env, id: u32) -> Circle {
    match env.storage().instance().get(&DataKey::Circle(id)) {
        Some(c) => c,
        None => panic_with_error!(env, Error::CircleNotFound),
    }
}

fn write_circle(env: &Env, id: u32, circle: &Circle) {
    env.storage().instance().set(&DataKey::Circle(id), circle);
}

fn next_circle_id(env: &Env) -> u32 {
    let key = DataKey::CircleCount;
    let current: u32 = env.storage().instance().get(&key).unwrap_or(0);
    let next = current + 1;
    env.storage().instance().set(&key, &next);
    next
}

#[contractimpl]
impl SoroSusu {
    // ============================================================
    // GLOBAL & FEE LOGIC
    // ============================================================

    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&ADMIN_KEY) {
            return Err(Error::Unauthorized);
        }

        env.storage().instance().set(&ADMIN_KEY, &admin);
        env.storage().instance().set(&FEE_BASIS_POINTS_KEY, &0u32);
        env.storage()
            .instance()
            .set(&MEMBERS_KEY, &Vec::<Address>::new(&env));
        env.storage()
            .instance()
            .set(&CONTRIBS_KEY, &Map::<Address, i128>::new(&env));
        env.storage()
            .instance()
            .set(&USER_BAL_KEY, &Map::<Address, i128>::new(&env));
        Self::touch_last_active(&env);

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

        env.storage()
            .instance()
            .set(&FEE_BASIS_POINTS_KEY, &fee_basis_points);
        env.storage().instance().set(&TREASURY_KEY, &treasury);
        Self::touch_last_active(&env);
        Ok(())
    }

    pub fn deposit(env: Env, user: Address, token: Address, amount: i128) -> Result<(), Error> {
        user.require_auth();

        let contract_address = env.current_contract_address();
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&user, &contract_address, &amount);

        let mut balances: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&USER_BAL_KEY)
            .unwrap_or(Map::new(&env));
        let current = balances.get(user.clone()).unwrap_or(0);
        balances.set(user, current + amount);
        env.storage().instance().set(&USER_BAL_KEY, &balances);

        Ok(())
    }

    pub fn emergency_withdraw(env: Env, user: Address, token: Address) -> Result<(), Error> {
        user.require_auth();

        let last_active = Self::get_last_active_timestamp(env.clone());
        let now = env.ledger().timestamp();
        let unlock_at = last_active.saturating_add(EMERGENCY_WITHDRAWAL_DELAY_SECS);
        if now <= unlock_at {
            return Err(Error::EmergencyWithdrawalNotAvailable);
        }

        let mut balances: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&USER_BAL_KEY)
            .unwrap_or(Map::new(&env));
        let amount = balances.get(user.clone()).unwrap_or(0);
        if amount > 0 {
            let contract_address = env.current_contract_address();
            let token_client = token::Client::new(&env, &token);
            token_client.transfer(&contract_address, &user, &amount);
        }
        balances.remove(user.clone());
        env.storage().instance().set(&USER_BAL_KEY, &balances);

        Ok(())
    }

    pub fn admin_action(env: Env) -> Result<(), Error> {
        Self::require_admin(&env)?;
        Self::touch_last_active(&env);
        Ok(())
    }

    pub fn kick_member(
        env: Env,
        token: Address,
        member: Address,
        penalty: i128,
    ) -> Result<(), Error> {
        Self::require_admin(&env)?;

        let mut members: Vec<Address> = env
            .storage()
            .instance()
            .get(&MEMBERS_KEY)
            .unwrap_or(Vec::new(&env));
        let index = Self::find_member_index(&members, &member).ok_or(Error::MemberNotFound)?;

        let mut contribs: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&CONTRIBS_KEY)
            .unwrap_or(Map::new(&env));
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

        env.events()
            .publish((symbol_short!("Kicked"), member.clone()), (refund, penalty));
        Self::touch_last_active(&env);

        Ok(())
    }

    pub fn swap_member(env: Env, old_member: Address, new_member: Address) -> Result<(), Error> {
        old_member.require_auth();
        new_member.require_auth();
        Self::apply_member_swap(&env, old_member, new_member)
    }

    pub fn swap_member_by_admin(
        env: Env,
        old_member: Address,
        new_member: Address,
    ) -> Result<(), Error> {
        Self::require_admin(&env)?;
        let result = Self::apply_member_swap(&env, old_member, new_member);
        if result.is_ok() {
            Self::touch_last_active(&env);
        }
        result
    }

    pub fn compute_and_transfer_payout(
        env: Env,
        token: Address,
        from: Address,
        recipient: Address,
        gross_payout: i128,
    ) -> Result<(), Error> {
        let fee_bps = env
            .storage()
            .instance()
            .get::<_, u32>(&FEE_BASIS_POINTS_KEY)
            .unwrap_or(0);

        let fee = if fee_bps == 0 {
            0_i128
        } else {
            (gross_payout * fee_bps as i128) / MAX_BASIS_POINTS as i128
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

    // ============================================================
    // CIRCLE MANAGEMENT & GOVERNANCE
    // ============================================================

    pub fn create_circle(env: Env, admin: Address, contribution: i128, is_random_queue: bool) -> u32 {
        admin.require_auth();
        let id = next_circle_id(&env);

        let circle = Circle {
            admin,
            contribution,
            members: Vec::new(&env),
            is_random_queue,
            payout_queue: Vec::new(&env),
            has_received_payout: Vec::new(&env),
            cycle_number: 1,
            current_payout_index: 0,
            total_volume_distributed: 0,
            is_dissolved: false,
            dissolution_votes: Vec::new(&env),
            contributions_paid: Vec::new(&env),
        };

        write_circle(&env, id, &circle);
        id
    }

    pub fn join_circle(env: Env, invoker: Address, circle_id: u32) {
        invoker.require_auth();
        let mut circle = read_circle(&env, circle_id);

        if circle.is_dissolved {
            panic_with_error!(&env, Error::AlreadyDissolved);
        }

        for member in circle.members.iter() {
            if member == invoker {
                panic_with_error!(&env, Error::AlreadyJoined);
            }
        }

        let member_count: u32 = circle.members.len();
        if member_count >= MAX_MEMBERS {
            panic_with_error!(&env, Error::MaxMembersReached);
        }

        circle.members.push_back(invoker.clone());
        circle.has_received_payout.push_back(false);
        circle.contributions_paid.push_back(circle.contribution);

        write_circle(&env, circle_id, &circle);
    }

    pub fn finalize_circle(env: Env, admin: Address, circle_id: u32) {
        admin.require_auth();
        let mut circle = read_circle(&env, circle_id);

        if admin != circle.admin {
            panic_with_error!(&env, Error::Unauthorized);
        }

        if circle.is_dissolved {
            panic_with_error!(&env, Error::AlreadyDissolved);
        }

        if !circle.payout_queue.is_empty() {
            return; // Already finalized
        }

        if circle.is_random_queue {
            let mut shuffled = circle.members.clone();
            env.prng().shuffle(&mut shuffled);
            circle.payout_queue = shuffled;
        } else {
            circle.payout_queue = circle.members.clone();
        }

        write_circle(&env, circle_id, &circle);
    }

    pub fn process_payout(env: Env, admin: Address, circle_id: u32, recipient: Address) {
        admin.require_auth();
        let mut circle = read_circle(&env, circle_id);

        if admin != circle.admin {
            panic_with_error!(&env, Error::Unauthorized);
        }

        if circle.is_dissolved {
            panic_with_error!(&env, Error::AlreadyDissolved);
        }

        let mut member_index: Option<u32> = None;
        for (i, member) in circle.members.iter().enumerate() {
            if member == recipient {
                member_index = Some(i as u32);
                break;
            }
        }

        let index = match member_index {
            Some(i) => i,
            None => panic_with_error!(&env, Error::NotMember),
        };

        if circle.has_received_payout.get(index).unwrap_or(false) {
            panic_with_error!(&env, Error::PayoutAlreadyReceived);
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
            env.events().publish((symbol_short!("CYCLE_COMP"),), event);
        }

        write_circle(&env, circle_id, &circle);
    }

    pub fn rollover_group(env: Env, admin: Address, circle_id: u32) {
        admin.require_auth();
        let mut circle = read_circle(&env, circle_id);

        if admin != circle.admin {
            panic_with_error!(&env, Error::Unauthorized);
        }

        for received in circle.has_received_payout.iter() {
            if !received {
                panic_with_error!(&env, Error::CycleNotComplete);
            }
        }

        circle.cycle_number += 1;
        circle.current_payout_index = 0;
        circle.total_volume_distributed = 0;

        let len = circle.has_received_payout.len();
        circle.has_received_payout = Vec::new(&env);
        for _ in 0..len {
            circle.has_received_payout.push_back(false);
        }

        let event = GroupRolloverEvent {
            group_id: circle_id,
            new_cycle_number: circle.cycle_number,
        };
        env.events().publish((symbol_short!("GROUP_ROLL"),), event);

        write_circle(&env, circle_id, &circle);
    }

    pub fn propose_dissolution(env: Env, invoker: Address, circle_id: u32) {
        invoker.require_auth();
        let mut circle = read_circle(&env, circle_id);

        if circle.is_dissolved {
            panic_with_error!(&env, Error::AlreadyDissolved);
        }

        if !circle.members.contains(&invoker) {
            panic_with_error!(&env, Error::NotMember);
        }

        if !circle.dissolution_votes.contains(&invoker) {
            circle.dissolution_votes.push_back(invoker.clone());
        }

        write_circle(&env, circle_id, &circle);
    }

    pub fn vote_dissolve(env: Env, invoker: Address, circle_id: u32) {
        invoker.require_auth();
        let mut circle = read_circle(&env, circle_id);

        if circle.is_dissolved {
            panic_with_error!(&env, Error::AlreadyDissolved);
        }

        if !circle.members.contains(&invoker) {
            panic_with_error!(&env, Error::NotMember);
        }

        if circle.dissolution_votes.contains(&invoker) {
            panic_with_error!(&env, Error::AlreadyVoted);
        }

        circle.dissolution_votes.push_back(invoker);

        let total_members = circle.members.len();
        let votes = circle.dissolution_votes.len();

        if votes * 2 > total_members {
            circle.is_dissolved = true;
        }

        write_circle(&env, circle_id, &circle);
    }

    pub fn withdraw_pro_rata(env: Env, invoker: Address, circle_id: u32) -> i128 {
        invoker.require_auth();
        let mut circle = read_circle(&env, circle_id);

        if !circle.is_dissolved {
            panic_with_error!(&env, Error::NotDissolved);
        }

        let mut index = None;
        for (i, member) in circle.members.iter().enumerate() {
            if member == invoker {
                index = Some(i as u32);
                break;
            }
        }

        let i = index.unwrap_or_else(|| panic_with_error!(&env, Error::NotMember));
        let contributed = circle.contributions_paid.get(i).unwrap_or(0);
        
        let received = if circle.has_received_payout.get(i).unwrap_or(false) {
            circle.contribution
        } else {
            0
        };

        let refundable = contributed - received;

        if refundable > 0 {
            circle.contributions_paid.set(i, 0);
            write_circle(&env, circle_id, &circle);
        }

        refundable
    }

    // ============================================================
    // VIEW GETTERS & HELPERS
    // ============================================================

    pub fn get_last_active_timestamp(env: Env) -> u64 {
        env.storage().instance().get(&LAST_ACTIVE_KEY).unwrap_or(0)
    }

    pub fn get_user_balance(env: Env, user: Address) -> i128 {
        let balances: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&USER_BAL_KEY)
            .unwrap_or(Map::new(&env));
        balances.get(user).unwrap_or(0)
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

    pub fn get_circle(env: Env, circle_id: u32) -> Circle {
        read_circle(&env, circle_id)
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

    fn require_admin(env: &Env) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&ADMIN_KEY)
            .ok_or(Error::Unauthorized)?;
        admin.require_auth();
        Ok(())
    }

    fn apply_member_swap(env: &Env, old_member: Address, new_member: Address) -> Result<(), Error> {
        let members: Vec<Address> = env
            .storage()
            .instance()
            .get(&MEMBERS_KEY)
            .unwrap_or(Vec::new(env));

        let old_index =
            Self::find_member_index(&members, &old_member).ok_or(Error::MemberNotFound)?;
        if Self::find_member_index(&members, &new_member).is_some() && old_member != new_member {
            return Err(Error::MemberAlreadyExists);
        }

        let mut updated_members = Vec::new(env);
        for (index, member) in members.iter().enumerate() {
            if index as u32 == old_index {
                updated_members.push_back(new_member.clone());
            } else {
                updated_members.push_back(member);
            }
        }

        let mut contribs: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&CONTRIBS_KEY)
            .unwrap_or(Map::new(env));
        let total_contributed = contribs.get(old_member.clone()).unwrap_or(0);
        contribs.remove(old_member.clone());
        contribs.set(new_member.clone(), total_contributed);

        env.storage().instance().set(&MEMBERS_KEY, &updated_members);
        env.storage().instance().set(&CONTRIBS_KEY, &contribs);

        Ok(())
    }

    fn find_member_index(members: &Vec<Address>, target: &Address) -> Option<u32> {
        for (index, member) in members.iter().enumerate() {
            if member == *target {
                return Some(index as u32);
            }
        }
        None
    }

    fn touch_last_active(env: &Env) {
        let now = env.ledger().timestamp();
        env.storage().instance().set(&LAST_ACTIVE_KEY, &now);
    }
}

#[cfg(test)]
mod test {
    extern crate std;

    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation, Ledger},
        token, vec, IntoVal, Env,
    };

    fn create_token_contract<'a>(
        env: &Env,
        admin: &Address,
    ) -> (token::Client<'a>, token::StellarAssetClient<'a>) {
        let asset = env.register_stellar_asset_contract_v2(admin.clone());
        let contract_address = asset.address();
        (
            token::Client::new(env, &contract_address),
            token::StellarAssetClient::new(env, &contract_address),
        )
    }

    fn setup_global(env: &Env) -> (SoroSusuClient<'_>, Address) {
        let contract_id = env.register_contract(None, SoroSusu);
        let admin = Address::generate(env);
        let client = SoroSusuClient::new(env, &contract_id);
        (client, admin)
    }

    fn setup_circles() -> (Env, SoroSusuClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);
        (env, client)
    }

    fn seed_members_and_contribs(
        env: &Env,
        contract_id: &Address,
        members: Vec<Address>,
        contribs: Map<Address, i128>,
    ) {
        env.as_contract(contract_id, || {
            env.storage().instance().set(&MEMBERS_KEY, &members);
            env.storage().instance().set(&CONTRIBS_KEY, &contribs);
        });
    }

    fn read_members_and_contribs(
        env: &Env,
        contract_id: &Address,
    ) -> (Vec<Address>, Map<Address, i128>) {
        env.as_contract(contract_id, || {
            let stored_members: Vec<Address> = env.storage().instance().get(&MEMBERS_KEY).unwrap();
            let stored_contribs: Map<Address, i128> =
                env.storage().instance().get(&CONTRIBS_KEY).unwrap();
            (stored_members, stored_contribs)
        })
    }

    #[test]
    fn set_protocol_fee_rejects_over_max() {
        let env = Env::default();
        let (client, admin) = setup_global(&env);
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
        let (client, admin) = setup_global(&env);
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
    fn join_circle_enforces_max_members() {
        let (env, client) = setup_circles();
        let admin = Address::generate(&env);
        let circle_id = client.create_circle(&admin, &10_i128, &false);

        for _ in 0..MAX_MEMBERS {
            let member = Address::generate(&env);
            client.join_circle(&member, &circle_id);
        }

        let extra = Address::generate(&env);
        let result = std::panic::catch_unwind(|| {
            client.join_circle(&extra, &circle_id);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_random_queue_finalization() {
        let (env, client) = setup_circles();
        let admin = Address::generate(&env);
        let circle_id = client.create_circle(&admin, &10_i128, &true);

        let members: std::vec::Vec<Address> = (0..5).map(|_| Address::generate(&env)).collect();
        for member in &members {
            client.join_circle(member, &circle_id);
        }

        client.finalize_circle(&admin, &circle_id);
        let queue = client.get_payout_queue(&circle_id);

        assert_eq!(queue.len(), 5);
        for member in &members {
            assert!(queue.contains(member));
        }
    }

    #[test]
    fn test_sequential_queue_finalization() {
        let (env, client) = setup_circles();
        let admin = Address::generate(&env);
        let circle_id = client.create_circle(&admin, &10_i128, &false);

        let members: std::vec::Vec<Address> = (0..5).map(|_| Address::generate(&env)).collect();
        for member in &members {
            client.join_circle(member, &circle_id);
        }

        client.finalize_circle(&admin, &circle_id);
        let queue = client.get_payout_queue(&circle_id);

        assert_eq!(queue.len(), 5);
        for (i, member) in members.iter().enumerate() {
            assert_eq!(queue.get(i as u32), Some(member.clone()));
        }
    }

    #[test]
    fn test_process_payout_and_cycle_completion() {
        let (env, client) = setup_circles();
        let admin = Address::generate(&env);
        let circle_id = client.create_circle(&admin, &100_i128, &false);

        let members: std::vec::Vec<Address> = (0..3).map(|_| Address::generate(&env)).collect();
        for member in &members {
            client.join_circle(member, &circle_id);
        }

        client.finalize_circle(&admin, &circle_id);

        for member in &members {
            client.process_payout(&admin, &circle_id, member);
        }

        let (cycle_num, payout_index, total_volume) = client.get_cycle_info(&circle_id);
        assert_eq!(cycle_num, 1);
        assert_eq!(payout_index, 3);
        assert_eq!(total_volume, 300_i128);

        let events = env.events().all();
        assert!(!events.is_empty());
    }

    #[test]
    fn test_group_rollover() {
        let (env, client) = setup_circles();
        let admin = Address::generate(&env);
        let circle_id = client.create_circle(&admin, &50_i128, &false);

        let members: std::vec::Vec<Address> = (0..2).map(|_| Address::generate(&env)).collect();
        for member in &members {
            client.join_circle(member, &circle_id);
        }

        client.finalize_circle(&admin, &circle_id);

        for member in &members {
            client.process_payout(&admin, &circle_id, member);
        }

        client.rollover_group(&admin, &circle_id);

        let (cycle_num, payout_index, total_volume) = client.get_cycle_info(&circle_id);
        assert_eq!(cycle_num, 2);
        assert_eq!(payout_index, 0);
        assert_eq!(total_volume, 0_i128);
    }

    #[test]
    fn emergency_withdraw_after_seven_days() {
        let env = Env::default();
        env.mock_all_auths();

        let (client, admin) = setup_global(&env);
        let user = Address::generate(&env);
        let (token_client, token_admin) = create_token_contract(&env, &admin);
        token_admin.mint(&user, &1000);

        client.initialize(&admin);
        client.deposit(&user, &token_client.address, &500);
        assert_eq!(client.get_user_balance(&user), 500);

        env.ledger().with_mut(|li| {
            li.timestamp += EMERGENCY_WITHDRAWAL_DELAY_SECS + 1;
        });

        client.emergency_withdraw(&user, &token_client.address);
        assert_eq!(client.get_user_balance(&user), 0);
        assert_eq!(token_client.balance(&user), 1000);
    }

    #[test]
    fn swap_member_replaces_queue_spot_and_transfers_credit() {
        let env = Env::default();
        let (client, admin) = setup_global(&env);
        client.initialize(&admin);

        let old_member = Address::generate(&env);
        let new_member = Address::generate(&env);
        let other_member = Address::generate(&env);

        let mut members = Vec::new(&env);
        members.push_back(old_member.clone());
        members.push_back(other_member.clone());

        let mut contribs = Map::new(&env);
        contribs.set(old_member.clone(), 750_i128);
        contribs.set(other_member.clone(), 200_i128);
        seed_members_and_contribs(&env, &client.address, members, contribs);

        env.mock_all_auths();
        client.swap_member(&old_member, &new_member);

        let (stored_members, stored_contribs) = read_members_and_contribs(&env, &client.address);

        assert_eq!(stored_members.get(0).unwrap(), new_member.clone());
        assert_eq!(stored_members.get(1).unwrap(), other_member.clone());
        assert_eq!(stored_contribs.get(new_member.clone()).unwrap(), 750_i128);
        assert_eq!(stored_contribs.get(old_member.clone()), None);
    }
}