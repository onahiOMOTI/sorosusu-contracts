#![no_std]
use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, token, Address, Env, Vec,
};

// --- ERROR CODES ---

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    Unauthorized = 1,
    MemberNotFound = 2,
    CircleFull = 3,
    AlreadyMember = 4,
    CircleNotFound = 5,
    InvalidAmount = 6,
    RoundAlreadyFinalized = 7,
    RoundNotFinalized = 8,
    NotAllContributed = 9,
    PayoutNotScheduled = 10,
    PayoutTooEarly = 11,
    InsufficientInsurance = 12,
    InsuranceAlreadyUsed = 13,
    RateLimitExceeded = 14,
}

// --- CONSTANTS ---
const REFERRAL_DISCOUNT_BPS: u32 = 500; // 5%
const RATE_LIMIT_SECONDS: u64 = 300; // 5 minutes

// --- DATA STRUCTURES ---

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Circle(u64),
    Member(Address),
    CircleCount,
    Deposit(u64, Address),
    GroupReserve,
    ScheduledPayoutTime(u64),
    LastCreatedTimestamp(Address),
    SafetyDeposit(Address, u64),
    LendingPool,
    CircleMembership(u64, Address),
    CircleMemberIndex(u64, u32),
    SuccessionProposal(u64),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum MemberStatus {
    Active,
    AwaitingReplacement,
    Ejected,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub address: Address,
    pub index: u32,
    pub contribution_count: u32,
    pub last_contribution_time: u64,
    pub status: MemberStatus,
    pub tier_multiplier: u32,
    pub referrer: Option<Address>,
    pub buddy: Option<Address>,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CircleInfo {
    pub id: u64,
    pub creator: Address,
    pub current_lead: Address,
    pub organizer_fee_recipient: Address,
    pub contribution_amount: i128,
    pub max_members: u32,
    pub member_count: u32,
    pub current_recipient_index: u32,
    pub is_active: bool,
    pub token: Address,
    pub deadline_timestamp: u64,
    pub cycle_duration: u64,
    pub contribution_bitmap: u64,
    pub insurance_balance: i128,
    pub insurance_fee_bps: u32,
    pub is_insurance_used: bool,
    pub late_fee_bps: u32,
    pub nft_contract: Address,
    pub is_round_finalized: bool,
    pub current_pot_recipient: Option<Address>,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SuccessionProposal {
    pub nominee: Address,
    pub approvals_bitmap: u64,
}

// --- CONTRACT CLIENTS ---

#[contractclient(name = "SusuNftClient")]
pub trait SusuNftTrait {
    fn mint(env: Env, to: Address, token_id: u128);
    fn burn(env: Env, from: Address, token_id: u128);
}

#[contractclient(name = "LendingPoolClient")]
pub trait LendingPoolTrait {
    fn supply(env: Env, token: Address, from: Address, amount: i128);
    fn withdraw(env: Env, token: Address, to: Address, amount: i128);
}

// --- CONTRACT TRAIT ---

pub trait SoroSusuTrait {
    fn init(env: Env, admin: Address);
    fn set_lending_pool(env: Env, admin: Address, pool: Address);

    fn create_circle(
        env: Env,
        creator: Address,
        amount: i128,
        max_members: u32,
        token: Address,
        cycle_duration: u64,
        insurance_fee_bps: u32,
        nft_contract: Address,
    ) -> u64;

    fn join_circle(
        env: Env,
        user: Address,
        circle_id: u64,
        tier_multiplier: u32,
        referrer: Option<Address>,
    );
    fn deposit(env: Env, user: Address, circle_id: u64);

    fn finalize_round(env: Env, caller: Address, circle_id: u64);
    fn claim_pot(env: Env, user: Address, circle_id: u64);

    fn trigger_insurance_coverage(env: Env, caller: Address, circle_id: u64, member: Address);
    fn eject_member(env: Env, caller: Address, circle_id: u64, member: Address);

    fn propose_group_lead_succession(env: Env, user: Address, circle_id: u64, nominee: Address);
    fn approve_group_lead_succession(env: Env, user: Address, circle_id: u64);

    fn pair_with_member(env: Env, user: Address, buddy_address: Address);
    fn set_safety_deposit(env: Env, user: Address, circle_id: u64, amount: i128);

    fn get_circle(env: Env, circle_id: u64) -> CircleInfo;
    fn get_member(env: Env, circle_id: u64, member: Address) -> Member;
    fn get_members(env: Env, circle_id: u64) -> Vec<Address>;
    fn get_succession_proposal(env: Env, circle_id: u64) -> Option<SuccessionProposal>;
}

fn read_circle(env: &Env, circle_id: u64) -> CircleInfo {
    env.storage()
        .instance()
        .get(&DataKey::Circle(circle_id))
        .expect("Circle not found")
}

fn write_circle(env: &Env, circle: &CircleInfo) {
    env.storage()
        .instance()
        .set(&DataKey::Circle(circle.id), circle);
}

fn read_member(env: &Env, member: &Address) -> Member {
    env.storage()
        .instance()
        .get(&DataKey::Member(member.clone()))
        .expect("Member not found")
}

fn require_circle_membership(env: &Env, circle_id: u64, member: &Address) {
    if !env
        .storage()
        .instance()
        .has(&DataKey::CircleMembership(circle_id, member.clone()))
    {
        panic!("Member not found");
    }
}

fn read_circle_member_index(env: &Env, circle_id: u64, member: &Address) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::CircleMembership(circle_id, member.clone()))
        .expect("Member not found")
}

fn active_member_count(env: &Env, circle: &CircleInfo) -> u32 {
    let mut active_members = 0u32;

    for index in 0..circle.member_count {
        if let Some(member_address) = env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::CircleMemberIndex(circle.id, index))
        {
            let member = read_member(env, &member_address);
            if member.status == MemberStatus::Active {
                active_members += 1;
            }
        }
    }

    active_members
}

fn active_member_bitmap(env: &Env, circle: &CircleInfo) -> u64 {
    let mut bitmap = 0u64;

    for index in 0..circle.member_count {
        if let Some(member_address) = env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::CircleMemberIndex(circle.id, index))
        {
            let member = read_member(env, &member_address);
            if member.status == MemberStatus::Active {
                bitmap |= 1u64 << index;
            }
        }
    }

    bitmap
}

fn required_succession_approvals(active_members: u32) -> u32 {
    if active_members == 0 {
        0
    } else {
        ((active_members * 2) + 2) / 3
    }
}

fn maybe_execute_succession(env: &Env, circle: &mut CircleInfo, proposal: SuccessionProposal) {
    let active_members = active_member_count(env, circle);
    let active_votes = (proposal.approvals_bitmap & active_member_bitmap(env, circle)).count_ones();
    let threshold = required_succession_approvals(active_members);

    if active_votes >= threshold {
        circle.current_lead = proposal.nominee.clone();
        circle.organizer_fee_recipient = proposal.nominee;
        write_circle(env, circle);
        env.storage()
            .instance()
            .remove(&DataKey::SuccessionProposal(circle.id));
    } else {
        write_circle(env, circle);
        env.storage()
            .instance()
            .set(&DataKey::SuccessionProposal(circle.id), &proposal);
    }
}

fn find_active_member_address(env: &Env, circle: &CircleInfo, start_index: u32) -> Address {
    if circle.member_count == 0 {
        panic!("Member not found");
    }

    let base_index = start_index % circle.member_count;

    for offset in 0..circle.member_count {
        let index = (base_index + offset) % circle.member_count;
        if let Some(member_address) = env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::CircleMemberIndex(circle.id, index))
        {
            let member = read_member(env, &member_address);
            if member.status == MemberStatus::Active {
                return member_address;
            }
        }
    }

    panic!("Member not found");
}

fn next_active_member_index(env: &Env, circle: &CircleInfo, start_index: u32) -> u32 {
    if circle.member_count == 0 {
        return 0;
    }

    let base_index = start_index % circle.member_count;

    for offset in 0..circle.member_count {
        let index = (base_index + offset) % circle.member_count;
        if let Some(member_address) = env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::CircleMemberIndex(circle.id, index))
        {
            let member = read_member(env, &member_address);
            if member.status == MemberStatus::Active {
                return index;
            }
        }
    }

    0
}

// --- IMPLEMENTATION ---

#[contract]
pub struct SoroSusu;

#[contractimpl]
impl SoroSusuTrait for SoroSusu {
    fn init(env: Env, admin: Address) {
        if !env.storage().instance().has(&DataKey::CircleCount) {
            env.storage().instance().set(&DataKey::CircleCount, &0u64);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    fn set_lending_pool(env: Env, admin: Address, pool: Address) {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");

        if admin != stored_admin {
            panic!("Unauthorized");
        }

        env.storage().instance().set(&DataKey::LendingPool, &pool);
    }

    fn create_circle(
        env: Env,
        creator: Address,
        amount: i128,
        max_members: u32,
        token: Address,
        cycle_duration: u64,
        insurance_fee_bps: u32,
        nft_contract: Address,
    ) -> u64 {
        creator.require_auth();

        if amount <= 0 {
            panic!("Invalid amount");
        }

        if max_members == 0 || max_members > 64 {
            panic!("Max members must be between 1 and 64");
        }

        if insurance_fee_bps > 10_000 {
            panic!("Insurance fee cannot exceed 100%");
        }

        let current_time = env.ledger().timestamp();
        let rate_limit_key = DataKey::LastCreatedTimestamp(creator.clone());

        if let Some(last_created) = env
            .storage()
            .instance()
            .get::<DataKey, u64>(&rate_limit_key)
        {
            if current_time < last_created + RATE_LIMIT_SECONDS {
                panic!("Rate limit exceeded");
            }
        }

        env.storage().instance().set(&rate_limit_key, &current_time);

        let mut circle_count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::CircleCount)
            .unwrap_or(0);
        circle_count += 1;

        let new_circle = CircleInfo {
            id: circle_count,
            creator: creator.clone(),
            current_lead: creator.clone(),
            organizer_fee_recipient: creator,
            contribution_amount: amount,
            max_members,
            member_count: 0,
            current_recipient_index: 0,
            is_active: true,
            token,
            deadline_timestamp: current_time + cycle_duration,
            cycle_duration,
            contribution_bitmap: 0,
            insurance_balance: 0,
            insurance_fee_bps,
            is_insurance_used: false,
            late_fee_bps: 100, // 1%
            nft_contract,
            is_round_finalized: false,
            current_pot_recipient: None,
        };

        write_circle(&env, &new_circle);
        env.storage()
            .instance()
            .set(&DataKey::CircleCount, &circle_count);

        circle_count
    }

    fn join_circle(
        env: Env,
        user: Address,
        circle_id: u64,
        tier_multiplier: u32,
        referrer: Option<Address>,
    ) {
        user.require_auth();

        let mut circle = read_circle(&env, circle_id);
        if circle.member_count >= circle.max_members {
            panic!("Circle is full");
        }

        let member_key = DataKey::Member(user.clone());
        if env.storage().instance().has(&member_key) {
            panic!("Already member");
        }

        let new_member = Member {
            address: user.clone(),
            index: circle.member_count,
            contribution_count: 0,
            last_contribution_time: 0,
            status: MemberStatus::Active,
            tier_multiplier,
            referrer,
            buddy: None,
        };

        env.storage().instance().set(&member_key, &new_member);
        env.storage().instance().set(
            &DataKey::CircleMembership(circle_id, user.clone()),
            &new_member.index,
        );
        env.storage().instance().set(
            &DataKey::CircleMemberIndex(circle_id, new_member.index),
            &user,
        );

        circle.member_count += 1;
        write_circle(&env, &circle);

        let token_id = (circle_id as u128) << 64 | (new_member.index as u128);
        let nft_client = SusuNftClient::new(&env, &circle.nft_contract);
        nft_client.mint(&user, &token_id);
    }

    fn deposit(env: Env, user: Address, circle_id: u64) {
        user.require_auth();
        require_circle_membership(&env, circle_id, &user);

        let mut circle = read_circle(&env, circle_id);
        let member_key = DataKey::Member(user.clone());
        let mut member: Member = env
            .storage()
            .instance()
            .get(&member_key)
            .expect("Member not found");

        if member.status != MemberStatus::Active {
            panic!("Member not active");
        }

        let member_index = read_circle_member_index(&env, circle_id, &user);
        let current_time = env.ledger().timestamp();
        let base_amount = circle.contribution_amount * member.tier_multiplier as i128;
        let mut penalty_amount = 0i128;

        if current_time > circle.deadline_timestamp {
            let base_penalty = (base_amount * circle.late_fee_bps as i128) / 10000;
            let mut discount = 0i128;

            if let Some(ref_addr) = &member.referrer {
                let ref_key = DataKey::Member(ref_addr.clone());
                if env.storage().instance().has(&ref_key) {
                    discount = (base_penalty * REFERRAL_DISCOUNT_BPS as i128) / 10000;
                }
            }

            penalty_amount = base_penalty - discount;

            let mut reserve: i128 = env
                .storage()
                .instance()
                .get(&DataKey::GroupReserve)
                .unwrap_or(0);
            reserve += penalty_amount;
            env.storage()
                .instance()
                .set(&DataKey::GroupReserve, &reserve);
        }

        let insurance_fee = (base_amount * circle.insurance_fee_bps as i128) / 10000;
        let total_amount = base_amount + insurance_fee + penalty_amount;
        let token_client = token::Client::new(&env, &circle.token);

        let transfer_result =
            token_client.try_transfer(&user, &env.current_contract_address(), &total_amount);
        let transfer_success = match transfer_result {
            Ok(inner) => inner.is_ok(),
            Err(_) => false,
        };

        if !transfer_success {
            if let Some(buddy_addr) = &member.buddy {
                let safety_key = DataKey::SafetyDeposit(buddy_addr.clone(), circle_id);
                let safety_balance: i128 = env.storage().instance().get(&safety_key).unwrap_or(0);
                if safety_balance >= total_amount {
                    env.storage()
                        .instance()
                        .set(&safety_key, &(safety_balance - total_amount));
                } else {
                    panic!("Insufficient funds and buddy deposit");
                }
            } else {
                panic!("Insufficient funds");
            }
        }

        if insurance_fee > 0 {
            circle.insurance_balance += insurance_fee;
        }

        member.contribution_count += 1;
        member.last_contribution_time = current_time;
        circle.contribution_bitmap |= 1u64 << member_index;

        env.storage().instance().set(&member_key, &member);
        write_circle(&env, &circle);
    }

    fn finalize_round(env: Env, caller: Address, circle_id: u64) {
        caller.require_auth();

        let mut circle = read_circle(&env, circle_id);
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");

        if caller != circle.current_lead && caller != stored_admin {
            panic!("Unauthorized");
        }

        if circle.is_round_finalized {
            panic!("Round already finalized");
        }

        let expected_bitmap = active_member_bitmap(&env, &circle);
        if circle.contribution_bitmap != expected_bitmap {
            panic!("Not all contributed");
        }

        let recipient = find_active_member_address(&env, &circle, circle.current_recipient_index);
        circle.current_pot_recipient = Some(recipient);
        circle.is_round_finalized = true;

        env.storage().instance().set(
            &DataKey::ScheduledPayoutTime(circle_id),
            &env.ledger().timestamp(),
        );
        write_circle(&env, &circle);
    }

    fn claim_pot(env: Env, user: Address, circle_id: u64) {
        user.require_auth();

        let mut circle = read_circle(&env, circle_id);

        if !circle.is_round_finalized {
            panic!("Round not finalized");
        }

        if let Some(recipient) = &circle.current_pot_recipient {
            if user != *recipient {
                panic!("Unauthorized recipient");
            }
        } else {
            panic!("No recipient set");
        }

        let scheduled_time: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ScheduledPayoutTime(circle_id))
            .expect("Payout not scheduled");

        if env.ledger().timestamp() < scheduled_time {
            panic!("Payout too early");
        }

        let pot_amount = circle.contribution_amount * (circle.member_count as i128);
        let token_client = token::Client::new(&env, &circle.token);
        token_client.transfer(&env.current_contract_address(), &user, &pot_amount);

        circle.is_round_finalized = false;
        circle.contribution_bitmap = 0;
        circle.is_insurance_used = false;
        circle.current_pot_recipient = None;

        if circle.member_count > 0 {
            let next_index = (circle.current_recipient_index + 1) % circle.member_count;
            circle.current_recipient_index = next_active_member_index(&env, &circle, next_index);
        }

        write_circle(&env, &circle);
        env.storage()
            .instance()
            .remove(&DataKey::ScheduledPayoutTime(circle_id));
    }

    fn trigger_insurance_coverage(env: Env, caller: Address, circle_id: u64, member: Address) {
        caller.require_auth();
        require_circle_membership(&env, circle_id, &member);

        let mut circle = read_circle(&env, circle_id);
        if caller != circle.current_lead {
            panic!("Unauthorized");
        }

        if circle.is_insurance_used {
            panic!("Insurance already used");
        }

        let member_key = DataKey::Member(member.clone());
        let member_info: Member = env
            .storage()
            .instance()
            .get(&member_key)
            .expect("Member not found");

        if member_info.status != MemberStatus::Active {
            panic!("Member not active");
        }

        let member_index = read_circle_member_index(&env, circle_id, &member);
        if (circle.contribution_bitmap & (1u64 << member_index)) != 0 {
            panic!("Member already contributed");
        }

        let amount_needed = circle.contribution_amount * member_info.tier_multiplier as i128;
        if circle.insurance_balance < amount_needed {
            panic!("Insufficient insurance");
        }

        circle.contribution_bitmap |= 1u64 << member_index;
        circle.insurance_balance -= amount_needed;
        circle.is_insurance_used = true;

        write_circle(&env, &circle);
    }

    fn eject_member(env: Env, caller: Address, circle_id: u64, member: Address) {
        caller.require_auth();
        require_circle_membership(&env, circle_id, &member);

        let circle = read_circle(&env, circle_id);
        if caller != circle.current_lead {
            panic!("Unauthorized");
        }

        let member_key = DataKey::Member(member.clone());
        let mut member_info: Member = env
            .storage()
            .instance()
            .get(&member_key)
            .expect("Member not found");

        if member_info.status == MemberStatus::Ejected {
            panic!("Already ejected");
        }

        member_info.status = MemberStatus::Ejected;
        env.storage().instance().set(&member_key, &member_info);

        let member_index = read_circle_member_index(&env, circle_id, &member);
        let nft_client = SusuNftClient::new(&env, &circle.nft_contract);
        let token_id = (circle_id as u128) << 64 | (member_index as u128);
        nft_client.burn(&member, &token_id);
    }

    fn propose_group_lead_succession(env: Env, user: Address, circle_id: u64, nominee: Address) {
        user.require_auth();
        require_circle_membership(&env, circle_id, &user);

        let member = read_member(&env, &user);
        if member.status != MemberStatus::Active {
            panic!("Member not active");
        }

        let member_index = read_circle_member_index(&env, circle_id, &user);
        let mut circle = read_circle(&env, circle_id);
        let proposal = SuccessionProposal {
            nominee,
            approvals_bitmap: 1u64 << member_index,
        };

        maybe_execute_succession(&env, &mut circle, proposal);
    }

    fn approve_group_lead_succession(env: Env, user: Address, circle_id: u64) {
        user.require_auth();
        require_circle_membership(&env, circle_id, &user);

        let member = read_member(&env, &user);
        if member.status != MemberStatus::Active {
            panic!("Member not active");
        }

        let member_index = read_circle_member_index(&env, circle_id, &user);
        let mut circle = read_circle(&env, circle_id);
        let mut proposal: SuccessionProposal = env
            .storage()
            .instance()
            .get(&DataKey::SuccessionProposal(circle_id))
            .expect("No active succession proposal");

        proposal.approvals_bitmap |= 1u64 << member_index;
        maybe_execute_succession(&env, &mut circle, proposal);
    }

    fn pair_with_member(env: Env, user: Address, buddy_address: Address) {
        user.require_auth();

        let user_key = DataKey::Member(user.clone());
        let mut user_info: Member = env
            .storage()
            .instance()
            .get(&user_key)
            .expect("Member not found");

        user_info.buddy = Some(buddy_address);
        env.storage().instance().set(&user_key, &user_info);
    }

    fn set_safety_deposit(env: Env, user: Address, circle_id: u64, amount: i128) {
        user.require_auth();

        let circle = read_circle(&env, circle_id);
        let token_client = token::Client::new(&env, &circle.token);
        token_client.transfer(&user, &env.current_contract_address(), &amount);

        let safety_key = DataKey::SafetyDeposit(user.clone(), circle_id);
        let mut balance: i128 = env.storage().instance().get(&safety_key).unwrap_or(0);
        balance += amount;
        env.storage().instance().set(&safety_key, &balance);
    }

    fn get_circle(env: Env, circle_id: u64) -> CircleInfo {
        read_circle(&env, circle_id)
    }

    fn get_member(env: Env, circle_id: u64, member: Address) -> Member {
        require_circle_membership(&env, circle_id, &member);
        read_member(&env, &member)
    }

    fn get_members(env: Env, circle_id: u64) -> Vec<Address> {
        let circle = read_circle(&env, circle_id);
        let mut members = Vec::new(&env);

        for index in 0..circle.member_count {
            if let Some(member_address) = env
                .storage()
                .instance()
                .get::<DataKey, Address>(&DataKey::CircleMemberIndex(circle_id, index))
            {
                members.push_back(member_address);
            }
        }

        members
    }

    fn get_succession_proposal(env: Env, circle_id: u64) -> Option<SuccessionProposal> {
        read_circle(&env, circle_id);
        env.storage()
            .instance()
            .get(&DataKey::SuccessionProposal(circle_id))
    }
}
