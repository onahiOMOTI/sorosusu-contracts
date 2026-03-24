#![no_std]
use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, symbol_short, token,
    Address, Env, String, Symbol, Vec,
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
    InsufficientCollateral = 15,
    CollateralAlreadyStaked = 16,
    CollateralNotStaked = 17,
    CollateralLocked = 18,
    MemberNotDefaulted = 19,
    CollateralAlreadyReleased = 20,
    LeniencyRequestNotFound = 21,
    AlreadyVoted = 22,
    VotingPeriodExpired = 23,
    LeniencyAlreadyApproved = 24,
    LeniencyNotRequested = 25,
    CannotVoteForOwnRequest = 26,
    InvalidVote = 27,
    ProposalNotFound = 28,
    ProposalAlreadyExecuted = 29,
    VotingNotActive = 30,
    InsufficientVotingPower = 31,
    QuadraticVoteExceeded = 32,
    InvalidProposalType = 33,
    QuorumNotMet = 34,
    ProposalExpired = 35,
    CannotVouchForSelf = 36,
    InsufficientTrustScore = 37,
    VoucherNotActive = 38,
    VoucheeAlreadyMember = 39,
    VouchAlreadyExists = 40,
    VouchExpired = 41,
    CollateralInsufficientForVouch = 42,
}

// --- CONSTANTS ---
const REFERRAL_DISCOUNT_BPS: u32 = 500; // 5%
const RATE_LIMIT_SECONDS: u64 = 300; // 5 minutes
const LENIENCY_GRACE_PERIOD: u64 = 172800; // 48 hours in seconds
const VOTING_PERIOD: u64 = 86400; // 24 hours voting period
const MINIMUM_VOTING_PARTICIPATION: u32 = 50; // 50% minimum participation
const SIMPLE_MAJORITY_THRESHOLD: u32 = 51; // 51% simple majority
const QUADRATIC_VOTING_PERIOD: u64 = 604800; // 7 days for rule changes
const QUADRATIC_QUORUM: u32 = 40; // 40% quorum for quadratic voting
const QUADRATIC_MAJORITY: u32 = 60; // 60% supermajority for rule changes
const MAX_VOTE_WEIGHT: u32 = 100; // Maximum quadratic vote weight
const MIN_GROUP_SIZE_FOR_QUADRATIC: u32 = 10; // Enable quadratic voting for groups >= 10 members
const DEFAULT_COLLATERAL_BPS: u32 = 2000; // 20%
const HIGH_VALUE_THRESHOLD: i128 = 1_000_000_0; // 1000 XLM (assuming 7 decimals)
const MIN_TRUST_SCORE_FOR_VOUCH: u32 = 70; // Minimum trust score to vouch
const VOUCH_COLLATERAL_MULTIPLIER: u32 = 1500; // 15% of cycle value as vouch collateral
const VOUCH_EXPIRY_SECONDS: u64 = 2592000; // 30 days
const MAX_VOUCHES_PER_MEMBER: u32 = 3; // Maximum concurrent vouches
const INACTIVITY_THRESHOLD_MONTHS: u64 = 18; // 18 months inactivity threshold
const DECAY_PERCENTAGE_PER_MONTH: u32 = 50; // 5% decay per month (50 basis points)
const SECONDS_PER_MONTH: u64 = 2592000; // 30 days in seconds

// --- MILESTONE CONSTANTS ---
const CONSECUTIVE_ON_TIME_BONUS_5: u32 = 10; // 10 points for 5 consecutive on-time payments
const CONSECUTIVE_ON_TIME_BONUS_10: u32 = 25; // 25 points for 10 consecutive on-time payments
const CONSECUTIVE_ON_TIME_BONUS_12: u32 = 40; // 40 points for 12 consecutive on-time payments (full cycle)
const FIRST_GROUP_ORGANIZED_BONUS: u32 = 15; // 15 points for organizing first group
const PERFECT_ATTENDANCE_BONUS: u32 = 20; // 20 points for perfect attendance in a cycle
const EARLY_BIRD_STREAK_BONUS: u32 = 5; // 5 points for 3 consecutive early payments
const REFERRAL_MASTER_BONUS: u32 = 8; // 8 points for 3 successful referrals
const VOUCHING_CHAMPION_BONUS: u32 = 12; // 12 points for 5 successful vouches
const COMMUNITY_LEADER_BONUS: u32 = 18; // 18 points for high participation in voting
const RELIABILITY_STAR_BONUS: u32 = 30; // 30 points for 100% reliability over 6 months

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
    CollateralVault(Address, u64),
    CollateralConfig(u64),
    DefaultedMembers(u64),
    LeniencyRequest(u64, Address),
    LeniencyVotes(u64, Address, Address),
    SocialCapital(Address, u64),
    LeniencyStats(u64),
    Proposal(u64),
    QuadraticVote(u64, Address),
    VotingPower(Address, u64),
    ProposalStats(u64),
    VouchRecord(Address, Address), // voucher -> vouchee
    VouchCollateral(Address, u64), // vouchee -> vouch_id
    VouchStats(Address), // voucher stats
    VouchReverseMapping(Address, u64), // vouchee -> voucher (for efficient lookup)
    LastActivityTimestamp(Address), // Track user's last activity for reputation decay
    DecayHistory(Address, u64), // Track decay applications per user per circle
    MilestoneProgress(Address, u64), // Track milestone progress per user per circle
    MilestoneBonuses(Address, u64), // Track earned milestone bonuses per user per circle
    MilestoneStats(u64), // Global milestone statistics per circle
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum MilestoneType {
    ConsecutiveOnTimePayments,
    FirstGroupOrganized,
    PerfectAttendance,
    EarlyBirdStreak,
    ReferralMaster,
    VouchingChampion,
    CommunityLeader,
    ReliabilityStar,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum MilestoneStatus {
    InProgress,
    Completed,
    Claimed,
}

#[contracttype]
#[derive(Clone)]
pub struct MilestoneProgress {
    pub member: Address,
    pub circle_id: u64,
    pub milestone_type: MilestoneType,
    pub current_progress: u32,
    pub target_progress: u32,
    pub status: MilestoneStatus,
    pub start_timestamp: u64,
    pub completion_timestamp: Option<u64>,
}

#[contracttype]
#[derive(Clone)]
pub struct MilestoneBonus {
    pub member: Address,
    pub circle_id: u64,
    pub milestone_type: MilestoneType,
    pub bonus_points: u32,
    pub earned_timestamp: u64,
    pub is_applied: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct MilestoneStats {
    pub circle_id: u64,
    pub total_milestones_completed: u32,
    pub total_bonus_points_distributed: u32,
    pub members_with_milestones: u32,
    pub most_common_milestone: MilestoneType,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum MemberStatus {
    Active,
    AwaitingReplacement,
    Ejected,
    Defaulted,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum LeniencyVote {
    Approve,
    Reject,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum LeniencyRequestStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalType {
    ChangeLateFee,
    ChangeInsuranceFee,
    ChangeCycleDuration,
    AddMember,
    RemoveMember,
    ChangeQuorum,
    EmergencyAction,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalStatus {
    Draft,
    Active,
    Approved,
    Rejected,
    Executed,
    Expired,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum QuadraticVoteChoice {
    For,
    Against,
    Abstain,
}

#[contracttype]
#[derive(Clone)]
pub struct LeniencyRequest {
    pub requester: Address,
    pub circle_id: u64,
    pub request_timestamp: u64,
    pub voting_deadline: u64,
    pub status: LeniencyRequestStatus,
    pub approve_votes: u32,
    pub reject_votes: u32,
    pub total_votes_cast: u32,
    pub extension_hours: u64,
    pub reason: String,
}

#[contracttype]
#[derive(Clone)]
pub struct Proposal {
    pub id: u64,
    pub circle_id: u64,
    pub proposer: Address,
    pub proposal_type: ProposalType,
    pub title: String,
    pub description: String,
    pub created_timestamp: u64,
    pub voting_start_timestamp: u64,
    pub voting_end_timestamp: u64,
    pub status: ProposalStatus,
    pub for_votes: u64,
    pub against_votes: u64,
    pub total_voting_power: u64,
    pub quorum_met: bool,
    pub execution_data: String, // JSON or structured data for execution
}

#[contracttype]
#[derive(Clone)]
pub struct QuadraticVote {
    pub voter: Address,
    pub proposal_id: u64,
    pub vote_weight: u32,
    pub vote_choice: QuadraticVoteChoice,
    pub voting_power_used: u64,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct VotingPower {
    pub member: Address,
    pub circle_id: u64,
    pub token_balance: i128,
    pub quadratic_power: u64,
    pub last_updated: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct ProposalStats {
    pub total_proposals: u32,
    pub approved_proposals: u32,
    pub rejected_proposals: u32,
    pub executed_proposals: u32,
    pub average_participation: u32,
    pub average_voting_time: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum VouchStatus {
    Active,
    Slashed,
    Completed,
    Expired,
}

#[contracttype]
#[derive(Clone)]
pub struct VouchRecord {
    pub voucher: Address,
    pub vouchee: Address,
    pub circle_id: u64,
    pub collateral_amount: i128,
    pub vouch_timestamp: u64,
    pub expiry_timestamp: u64,
    pub status: VouchStatus,
    pub slash_count: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct VouchStats {
    pub voucher: Address,
    pub total_vouches_made: u32,
    pub active_vouches: u32,
    pub successful_vouches: u32,
    pub slashed_vouches: u32,
    pub total_collateral_locked: i128,
    pub total_collateral_lost: i128,
}

#[contracttype]
#[derive(Clone)]
pub struct LeniencyStats {
    pub total_requests: u32,
    pub approved_requests: u32,
    pub rejected_requests: u32,
    pub expired_requests: u32,
    pub average_participation: u32,
pub enum CollateralStatus {
    NotStaked,
    Staked,
    Slashed,
    Released,
}

#[contracttype]
#[derive(Clone)]
pub struct SocialCapital {
    pub member: Address,
    pub circle_id: u64,
    pub leniency_given: u32,
    pub leniency_received: u32,
    pub voting_participation: u32,
    pub trust_score: u32,
    pub last_activity_timestamp: u64,
    pub decay_count: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct CollateralInfo {
    pub member: Address,
    pub circle_id: u64,
    pub amount: i128,
    pub status: CollateralStatus,
    pub staked_timestamp: u64,
    pub release_timestamp: Option<u64>,
}

#[contracttype]
#[derive(Clone)]
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
#[derive(Clone)]
pub struct CircleInfo {
    pub id: u64,
    pub creator: Address,
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
    pub leniency_enabled: bool,
    pub grace_period_end: Option<u64>,
    pub quadratic_voting_enabled: bool,
    pub proposal_count: u64,
    pub requires_collateral: bool,
    pub collateral_bps: u32,
    pub total_cycle_value: i128,
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

    fn join_circle(env: Env, user: Address, circle_id: u64, tier_multiplier: u32, referrer: Option<Address>);
    fn deposit(env: Env, user: Address, circle_id: u64);
    
    fn finalize_round(env: Env, caller: Address, circle_id: u64);
    fn claim_pot(env: Env, user: Address, circle_id: u64);
    
    fn trigger_insurance_coverage(env: Env, caller: Address, circle_id: u64, member: Address);
    fn eject_member(env: Env, caller: Address, circle_id: u64, member: Address);
    
    fn pair_with_member(env: Env, user: Address, buddy_address: Address);
    fn set_safety_deposit(env: Env, user: Address, circle_id: u64, amount: i128);
    
    // Leniency voting functions
    fn request_leniency(env: Env, requester: Address, circle_id: u64, reason: String);
    fn vote_on_leniency(env: Env, voter: Address, circle_id: u64, requester: Address, vote: LeniencyVote);
    fn finalize_leniency_vote(env: Env, caller: Address, circle_id: u64, requester: Address);
    fn get_leniency_request(env: Env, circle_id: u64, requester: Address) -> LeniencyRequest;
    fn get_social_capital(env: Env, member: Address, circle_id: u64) -> SocialCapital;
    fn get_leniency_stats(env: Env, circle_id: u64) -> LeniencyStats;
    
    // Quadratic voting functions
    fn create_proposal(
        env: Env,
        proposer: Address,
        circle_id: u64,
        proposal_type: ProposalType,
        title: String,
        description: String,
        execution_data: String,
    ) -> u64;
    
    fn quadratic_vote(env: Env, voter: Address, proposal_id: u64, vote_weight: u32, vote_choice: QuadraticVoteChoice);
    fn execute_proposal(env: Env, caller: Address, proposal_id: u64);
    fn get_proposal(env: Env, proposal_id: u64) -> Proposal;
    fn get_voting_power(env: Env, member: Address, circle_id: u64) -> VotingPower;
    fn get_proposal_stats(env: Env, circle_id: u64) -> ProposalStats;
    fn update_voting_power(env: Env, member: Address, circle_id: u64, token_balance: i128);
    // Collateral functions
    fn stake_collateral(env: Env, user: Address, circle_id: u64, amount: i128);
    fn slash_collateral(env: Env, caller: Address, circle_id: u64, member: Address);
    fn release_collateral(env: Env, caller: Address, circle_id: u64, member: Address);
    fn mark_member_defaulted(env: Env, caller: Address, circle_id: u64, member: Address);
    
    // Social vouching functions
    fn vouch_for_member(env: Env, voucher: Address, vouchee: Address, circle_id: u64, collateral_amount: i128);
    fn slash_vouch_collateral(env: Env, caller: Address, circle_id: u64, vouchee: Address);
    fn release_vouch_collateral(env: Env, caller: Address, circle_id: u64, vouchee: Address);
    fn get_vouch_record(env: Env, voucher: Address, vouchee: Address) -> VouchRecord;
    fn get_vouch_stats(env: Env, voucher: Address) -> VouchStats;
    
    // Reputation decay functions
    fn apply_reputation_decay(env: Env, member: Address, circle_id: u64);
    fn update_activity_timestamp(env: Env, member: Address, circle_id: u64);
    fn get_reputation_with_decay(env: Env, member: Address, circle_id: u64) -> SocialCapital;
    
    // Milestone-based reputation boost functions
    fn update_milestone_progress(env: Env, member: Address, circle_id: u64, milestone_type: MilestoneType, progress_increment: u32);
    fn check_and_award_milestones(env: Env, member: Address, circle_id: u64);
    fn apply_milestone_bonus(env: Env, member: Address, circle_id: u64);
    fn get_milestone_progress(env: Env, member: Address, circle_id: u64, milestone_type: MilestoneType) -> MilestoneProgress;
    fn get_milestone_bonuses(env: Env, member: Address, circle_id: u64) -> Vec<MilestoneBonus>;
    fn get_milestone_stats(env: Env, circle_id: u64) -> MilestoneStats;
    fn calculate_total_reputation_boost(env: Env, member: Address, circle_id: u64) -> u32;
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
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).expect("Not initialized");
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

        // Rate limiting
        let current_time = env.ledger().timestamp();
        let rate_limit_key = DataKey::LastCreatedTimestamp(creator.clone());
        if let Some(last_created) = env.storage().instance().get::<DataKey, u64>(&rate_limit_key) {
            if current_time < last_created + RATE_LIMIT_SECONDS {
                panic!("Rate limit exceeded");
            }
        }
        env.storage().instance().set(&rate_limit_key, &current_time);

        let mut circle_count: u64 = env.storage().instance().get(&DataKey::CircleCount).unwrap_or(0);
        circle_count += 1;

        // Calculate total cycle value and determine collateral requirements
        let total_cycle_value = amount * (max_members as i128);
        let requires_collateral = total_cycle_value >= HIGH_VALUE_THRESHOLD;
        let collateral_bps = if requires_collateral { DEFAULT_COLLATERAL_BPS } else { 0 };

        let new_circle = CircleInfo {
            id: circle_count,
            creator: creator.clone(),
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
            leniency_enabled: true,
            grace_period_end: None,
            quadratic_voting_enabled: max_members >= MIN_GROUP_SIZE_FOR_QUADRATIC,
            proposal_count: 0,
            requires_collateral,
            collateral_bps,
            total_cycle_value,
        };

        env.storage().instance().set(&DataKey::Circle(circle_count), &new_circle);
        env.storage().instance().set(&DataKey::CircleCount, &circle_count);

        circle_count
    }

    fn join_circle(env: Env, user: Address, circle_id: u64, tier_multiplier: u32, referrer: Option<Address>) {
        user.require_auth();

        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        if circle.member_count >= circle.max_members {
            panic!("Circle is full");
        }

        let member_key = DataKey::Member(user.clone());
        if env.storage().instance().has(&member_key) {
            panic!("Already member");
        }

        // Check collateral requirement for high-value circles
        if circle.requires_collateral {
            // First check if member is vouched for
            let vouch_reverse_key = DataKey::VouchReverseMapping(user.clone(), circle_id);
            if let Some(_voucher) = env.storage().instance().get::<DataKey, Address>(&vouch_reverse_key) {
                // User is vouched for, skip collateral requirement
                let vouch_collateral_key = DataKey::VouchCollateral(user.clone(), circle_id);
                if let Some(_vouch_id) = env.storage().instance().get::<DataKey, u64>(&vouch_collateral_key) {
                    // Vouch exists, proceed without collateral check
                } else {
                    panic!("Vouch not found for this user");
                }
            } else {
                // No vouch found, check regular collateral
                let collateral_key = DataKey::CollateralVault(user.clone(), circle_id);
                let collateral_info: Option<CollateralInfo> = env.storage().instance().get(&collateral_key);
                
                match collateral_info {
                    Some(collateral) => {
                        if collateral.status != CollateralStatus::Staked {
                            panic!("Collateral not properly staked");
                        }
                    }
                    None => panic!("Collateral required for this circle"),
                }
            }
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
        circle.member_count += 1;
        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);

        // Mint NFT
        let token_id = (circle_id as u128) << 64 | (new_member.index as u128);
        let nft_client = SusuNftClient::new(&env, &circle.nft_contract);
        nft_client.mint(&user, &token_id);
    }

    fn deposit(env: Env, user: Address, circle_id: u64) {
        user.require_auth();

        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        let member_key = DataKey::Member(user.clone());
        let mut member: Member = env.storage().instance().get(&member_key).expect("Member not found");

        if member.status != MemberStatus::Active {
            panic!("Member not active");
        }

        let current_time = env.ledger().timestamp();
        let base_amount = circle.contribution_amount * member.tier_multiplier as i128;
        let mut penalty_amount = 0i128;

        // Check if late fee applies (considering grace periods)
        let effective_deadline = circle.grace_period_end.unwrap_or(circle.deadline_timestamp);
        
        if current_time > effective_deadline {
            let base_penalty = (base_amount * circle.late_fee_bps as i128) / 10000;
            // Apply referral discount
            let mut discount = 0i128;
            if let Some(ref_addr) = &member.referrer {
                let ref_key = DataKey::Member(ref_addr.clone());
                if env.storage().instance().has(&ref_key) {
                    discount = (base_penalty * REFERRAL_DISCOUNT_BPS as i128) / 10000;
                }
            }
            penalty_amount = base_penalty - discount;
            
            let mut reserve: i128 = env.storage().instance().get(&DataKey::GroupReserve).unwrap_or(0);
            reserve += penalty_amount;
            env.storage().instance().set(&DataKey::GroupReserve, &reserve);
        }

        let insurance_fee = (base_amount * circle.insurance_fee_bps as i128) / 10000;
        let total_amount = base_amount + insurance_fee + penalty_amount;

        let token_client = token::Client::new(&env, &circle.token);

        // Try transfer from user
        let transfer_result = token_client.try_transfer(&user, &env.current_contract_address(), &total_amount);
        let transfer_success = match transfer_result {
            Ok(inner) => inner.is_ok(),
            Err(_) => false,
        };

        if !transfer_success {
            // Buddy fallback
            if let Some(buddy_addr) = &member.buddy {
                let safety_key = DataKey::SafetyDeposit(buddy_addr.clone(), circle_id);
                let safety_balance: i128 = env.storage().instance().get(&safety_key).unwrap_or(0);
                if safety_balance >= total_amount {
                    env.storage().instance().set(&safety_key, &(safety_balance - total_amount));
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
        circle.contribution_bitmap |= 1 << member.index;
        
        env.storage().instance().set(&member_key, &member);
        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);
        
        // Update activity timestamp for reputation decay
        Self::update_activity_timestamp(&env, user, circle_id);
        
        // Check and award milestones based on this contribution
        Self::check_and_award_milestones(env, user.clone(), circle_id);
        
        // Apply any pending milestone bonuses
        Self::apply_milestone_bonus(env, user.clone(), circle_id);
    }

    fn finalize_round(env: Env, caller: Address, circle_id: u64) {
        caller.require_auth();
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).expect("Not initialized");
        if caller != circle.creator && caller != stored_admin {
            panic!("Unauthorized");
        }

        if circle.is_round_finalized {
            panic!("Round already finalized");
        }

        let expected_bitmap = (1u64 << circle.member_count) - 1;
        if circle.contribution_bitmap != expected_bitmap {
            panic!("Not all contributed");
        }

        // recipient is circle.current_recipient_index
        // We'll need a way to get member by index or store member addresses in circle.
        // For simplicity in this clean version, let's assume members are stored in a predictable way or we add member_addresses to CircleInfo.
        // Actually, let's use the bitmap and iterate to find the address if needed, or better, store it in storage under (circle_id, index)
    }

    fn claim_pot(env: Env, user: Address, circle_id: u64) {
        user.require_auth();
        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        
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

        let scheduled_time: u64 = env.storage().instance().get(&DataKey::ScheduledPayoutTime(circle_id)).expect("Payout not scheduled");
        if env.ledger().timestamp() < scheduled_time {
            panic!("Payout too early");
        }

        let pot_amount = circle.contribution_amount * (circle.member_count as i128);
        let token_client = token::Client::new(&env, &circle.token);
        token_client.transfer(&env.current_contract_address(), &user, &pot_amount);

        // Auto-release collateral if member has completed all contributions
        if circle.requires_collateral {
            let member_key = DataKey::Member(user.clone());
            if let Some(member_info) = env.storage().instance().get::<DataKey, Member>(&member_key) {
                if member_info.contribution_count >= circle.max_members {
                    let collateral_key = DataKey::CollateralVault(user.clone(), circle_id);
                    if let Some(mut collateral_info) = env.storage().instance().get::<DataKey, CollateralInfo>(&collateral_key) {
                        if collateral_info.status == CollateralStatus::Staked {
                            // Release collateral back to member
                            token_client.transfer(&env.current_contract_address(), &user, &collateral_info.amount);
                            
                            // Update collateral status
                            collateral_info.status = CollateralStatus::Released;
                            collateral_info.release_timestamp = Some(env.ledger().timestamp());
                            env.storage().instance().set(&collateral_key, &collateral_info);
                        }
                    }
                }
            }
        }

        // Reset for next round
        circle.is_round_finalized = false;
        circle.contribution_bitmap = 0;
        circle.is_insurance_used = false;
        circle.current_recipient_index = (circle.current_recipient_index + 1) % circle.member_count;
        circle.current_pot_recipient = None; // Should be set in finalize_round

        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);
        env.storage().instance().remove(&DataKey::ScheduledPayoutTime(circle_id));
    }

    fn trigger_insurance_coverage(env: Env, caller: Address, circle_id: u64, member: Address) {
        caller.require_auth();
        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        
        if caller != circle.creator {
            panic!("Unauthorized");
        }

        if circle.is_insurance_used {
            panic!("Insurance already used");
        }

        let member_key = DataKey::Member(member.clone());
        let member_info: Member = env.storage().instance().get(&member_key).expect("Member not found");
        
        let amount_needed = circle.contribution_amount * member_info.tier_multiplier as i128;
        if circle.insurance_balance < amount_needed {
            panic!("Insufficient insurance");
        }

        circle.contribution_bitmap |= 1 << member_info.index;
        circle.insurance_balance -= amount_needed;
        circle.is_insurance_used = true;

        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);
    }

    fn eject_member(env: Env, caller: Address, circle_id: u64, member: Address) {
        caller.require_auth();
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        
        if caller != circle.creator {
            panic!("Unauthorized");
        }

        let member_key = DataKey::Member(member.clone());
        let mut member_info: Member = env.storage().instance().get(&member_key).expect("Member not found");
        
        if member_info.status == MemberStatus::Ejected {
            panic!("Already ejected");
        }

        member_info.status = MemberStatus::Ejected;
        env.storage().instance().set(&member_key, &member_info);

        let nft_client = SusuNftClient::new(&env, &circle.nft_contract);
        let token_id = (circle_id as u128) << 64 | (member_info.index as u128);
        nft_client.burn(&member, &token_id);
    }

    fn pair_with_member(env: Env, user: Address, buddy_address: Address) {
        user.require_auth();
        let user_key = DataKey::Member(user.clone());
        let mut user_info: Member = env.storage().instance().get(&user_key).expect("Member not found");
        
        user_info.buddy = Some(buddy_address);
        env.storage().instance().set(&user_key, &user_info);
    }

    fn set_safety_deposit(env: Env, user: Address, circle_id: u64, amount: i128) {
        user.require_auth();
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        
        let token_client = token::Client::new(&env, &circle.token);
        token_client.transfer(&user, &env.current_contract_address(), &amount);

        let safety_key = DataKey::SafetyDeposit(user.clone(), circle_id);
        let mut balance: i128 = env.storage().instance().get(&safety_key).unwrap_or(0);
        balance += amount;
        env.storage().instance().set(&safety_key, &balance);
    }

    // --- LENIENCY VOTING IMPLEMENTATION ---

    fn request_leniency(env: Env, requester: Address, circle_id: u64, reason: String) {
        requester.require_auth();

        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        let member_key = DataKey::Member(requester.clone());
        let member_info: Member = env.storage().instance().get(&member_key).expect("Member not found");

        if member_info.status != MemberStatus::Active {
            panic!("Member not active");
        }

        // Check if there's already a pending request
        let request_key = DataKey::LeniencyRequest(circle_id, requester.clone());
        if let Some(existing_request) = env.storage().instance().get::<DataKey, LeniencyRequest>(&request_key) {
            if existing_request.status == LeniencyRequestStatus::Pending {
                panic!("Leniency request already pending");
            }
        }

        let current_time = env.ledger().timestamp();
        let voting_deadline = current_time + VOTING_PERIOD;

        let new_request = LeniencyRequest {
            requester: requester.clone(),
            circle_id,
            request_timestamp: current_time,
            voting_deadline,
            status: LeniencyRequestStatus::Pending,
            approve_votes: 0,
            reject_votes: 0,
            total_votes_cast: 0,
            extension_hours: 48, // 48 hours grace period
            reason,
        };

        env.storage().instance().set(&request_key, &new_request);

        // Update leniency stats
        let stats_key = DataKey::LeniencyStats(circle_id);
        let mut stats: LeniencyStats = env.storage().instance().get(&stats_key).unwrap_or(LeniencyStats {
            total_requests: 0,
            approved_requests: 0,
            rejected_requests: 0,
            expired_requests: 0,
            average_participation: 0,
        });
        stats.total_requests += 1;
        env.storage().instance().set(&stats_key, &stats);
    }

    fn vote_on_leniency(env: Env, voter: Address, circle_id: u64, requester: Address, vote: LeniencyVote) {
        voter.require_auth();

        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        let voter_key = DataKey::Member(voter.clone());
        let voter_info: Member = env.storage().instance().get(&voter_key).expect("Voter not found");

        if voter_info.status != MemberStatus::Active {
            panic!("Voter not active");
        }

        if voter == requester {
            panic!("Cannot vote for own request");
        }

        let request_key = DataKey::LeniencyRequest(circle_id, requester.clone());
        let mut request: LeniencyRequest = env.storage().instance().get(&request_key)
            .expect("Leniency request not found");

        if request.status != LeniencyRequestStatus::Pending {
            panic!("Voting period has ended");
        }

        let current_time = env.ledger().timestamp();
        if current_time > request.voting_deadline {
            request.status = LeniencyRequestStatus::Expired;
            env.storage().instance().set(&request_key, &request);
            panic!("Voting period expired");
        }

        // Check if already voted
        let vote_key = DataKey::LeniencyVotes(circle_id, voter.clone(), requester.clone());
        if env.storage().instance().has(&vote_key) {
            panic!("Already voted");
        }

        // Record the vote
        env.storage().instance().set(&vote_key, &vote);
        request.total_votes_cast += 1;

        match vote {
            LeniencyVote::Approve => request.approve_votes += 1,
            LeniencyVote::Reject => request.reject_votes += 1,
        }

        // Update social capital
        let social_capital_key = DataKey::SocialCapital(voter.clone(), circle_id);
        let mut social_capital: SocialCapital = env.storage().instance().get(&social_capital_key).unwrap_or(SocialCapital {
            member: voter.clone(),
            circle_id,
            leniency_given: 0,
            leniency_received: 0,
            voting_participation: 0,
            trust_score: 50, // Start with neutral score
            last_activity_timestamp: current_time,
            decay_count: 0,
        });
        social_capital.voting_participation += 1;
        
        // Update trust score based on voting patterns
        if vote == LeniencyVote::Approve {
            social_capital.leniency_given += 1;
            social_capital.trust_score = (social_capital.trust_score + 2).min(100); // Increase trust score
        } else {
            social_capital.trust_score = (social_capital.trust_score - 1).max(0); // Decrease trust score
        }
        
        env.storage().instance().set(&social_capital_key, &social_capital);
        
        // Update activity timestamp for reputation decay
        Self::update_activity_timestamp(&env, voter, circle_id);
        
        // Check and award milestones based on voting participation
        Self::check_and_award_milestones(env, voter.clone(), circle_id);
        
        // Apply any pending milestone bonuses
        Self::apply_milestone_bonus(env, voter.clone(), circle_id);

        // Check if voting should be finalized early (if majority reached)
        let total_possible_votes = (circle.member_count - 1) as u32; // Exclude requester
        let votes_needed_for_majority = (total_possible_votes * SIMPLE_MAJORITY_THRESHOLD) / 100;
        
        if request.approve_votes >= votes_needed_for_majority {
            request.status = LeniencyRequestStatus::Approved;
            self.finalize_leniency_vote_internal(&env, &circle_id, &requester, &mut request);
        } else if request.reject_votes >= votes_needed_for_majority {
            request.status = LeniencyRequestStatus::Rejected;
        }

        env.storage().instance().set(&request_key, &request);
    }

    fn finalize_leniency_vote(env: Env, caller: Address, circle_id: u64, requester: Address) {
        caller.require_auth();

        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        
        let request_key = DataKey::LeniencyRequest(circle_id, requester.clone());
        let mut request: LeniencyRequest = env.storage().instance().get(&request_key)
            .expect("Leniency request not found");

        if request.status != LeniencyRequestStatus::Pending {
            panic!("Request already finalized");
        }

        let current_time = env.ledger().timestamp();
        if current_time <= request.voting_deadline {
            panic!("Voting period not yet expired");
        }

        self.finalize_leniency_vote_internal(&env, &circle_id, &requester, &mut request);
        env.storage().instance().set(&request_key, &request);
    }

    fn finalize_leniency_vote_internal(&env: &Env, circle_id: &u64, requester: &Address, request: &mut LeniencyRequest) {
        // Calculate voting results
        let total_possible_votes = request.total_votes_cast;
        let minimum_participation = (total_possible_votes * MINIMUM_VOTING_PARTICIPATION) / 100;
        
        let mut final_status = LeniencyRequestStatus::Rejected;
        
        if request.total_votes_cast >= minimum_participation {
            let approval_percentage = (request.approve_votes * 100) / request.total_votes_cast;
            if approval_percentage >= SIMPLE_MAJORITY_THRESHOLD {
                final_status = LeniencyRequestStatus::Approved;
                
                // Apply grace period extension
                let circle_key = DataKey::Circle(*circle_id);
                let mut circle: CircleInfo = env.storage().instance().get(&circle_key).expect("Circle not found");
                
                let extension_seconds = request.extension_hours * 3600;
                let new_deadline = circle.deadline_timestamp + extension_seconds;
                circle.deadline_timestamp = new_deadline;
                circle.grace_period_end = Some(new_deadline);
                
                env.storage().instance().set(&circle_key, &circle);
                
                // Update requester's social capital
                let social_capital_key = DataKey::SocialCapital(requester.clone(), *circle_id);
                let mut social_capital: SocialCapital = env.storage().instance().get(&social_capital_key).unwrap_or(SocialCapital {
                    member: requester.clone(),
                    circle_id: *circle_id,
                    leniency_given: 0,
                    leniency_received: 0,
                    voting_participation: 0,
                    trust_score: 50,
                    last_activity_timestamp: env.ledger().timestamp(),
                    decay_count: 0,
                });
                social_capital.leniency_received += 1;
                social_capital.trust_score = (social_capital.trust_score + 5).min(100); // Bonus for receiving leniency
                env.storage().instance().set(&social_capital_key, &social_capital);
            }
        }
        
        request.status = final_status;

        // Update leniency stats
        let stats_key = DataKey::LeniencyStats(*circle_id);
        let mut stats: LeniencyStats = env.storage().instance().get(&stats_key).unwrap_or(LeniencyStats {
            total_requests: 0,
            approved_requests: 0,
            rejected_requests: 0,
            expired_requests: 0,
            average_participation: 0,
        });

        match final_status {
            LeniencyRequestStatus::Approved => stats.approved_requests += 1,
            LeniencyRequestStatus::Rejected => stats.rejected_requests += 1,
            LeniencyRequestStatus::Expired => stats.expired_requests += 1,
            _ => {}
        }

        // Update average participation
        if stats.total_requests > 0 {
            let total_participation = stats.average_participation * (stats.total_requests - 1) + request.total_votes_cast;
            stats.average_participation = total_participation / stats.total_requests;
        }

        env.storage().instance().set(&stats_key, &stats);
    }

    fn get_leniency_request(env: Env, circle_id: u64, requester: Address) -> LeniencyRequest {
        let request_key = DataKey::LeniencyRequest(circle_id, requester);
        env.storage().instance().get(&request_key).expect("Leniency request not found")
    }

    fn get_social_capital(env: Env, member: Address, circle_id: u64) -> SocialCapital {
        // Use the new decay-aware function
        Self::get_reputation_with_decay(env, member, circle_id)
    }

    fn get_leniency_stats(env: Env, circle_id: u64) -> LeniencyStats {
        let stats_key = DataKey::LeniencyStats(circle_id);
        env.storage().instance().get(&stats_key).unwrap_or(LeniencyStats {
            total_requests: 0,
            approved_requests: 0,
            rejected_requests: 0,
            expired_requests: 0,
            average_participation: 0,
        })
    }

    // --- QUADRATIC VOTING IMPLEMENTATION ---

    fn create_proposal(
        env: Env,
        proposer: Address,
        circle_id: u64,
        proposal_type: ProposalType,
        title: String,
        description: String,
        execution_data: String,
    ) -> u64 {
        proposer.require_auth();

        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        
        if !circle.quadratic_voting_enabled {
            panic!("Quadratic voting not enabled for this circle");
        }

        let member_key = DataKey::Member(proposer.clone());
        let member_info: Member = env.storage().instance().get(&member_key).expect("Member not found");

        if member_info.status != MemberStatus::Active {
            panic!("Member not active");
        }

        let current_time = env.ledger().timestamp();
        let mut proposal_count: u64 = env.storage().instance().get(&DataKey::CircleCount).unwrap_or(0);
        proposal_count += 1;

        let new_proposal = Proposal {
            id: proposal_count,
            circle_id,
            proposer: proposer.clone(),
            proposal_type,
            title,
            description,
            created_timestamp: current_time,
            voting_start_timestamp: current_time,
            voting_end_timestamp: current_time + QUADRATIC_VOTING_PERIOD,
            status: ProposalStatus::Active,
            for_votes: 0,
            against_votes: 0,
            total_voting_power: 0,
            quorum_met: false,
            execution_data,
        };

        env.storage().instance().set(&DataKey::Proposal(proposal_count), &new_proposal);

        // Update circle proposal count
        let mut circle_info = circle;
        circle_info.proposal_count += 1;
        env.storage().instance().set(&DataKey::Circle(circle_id), &circle_info);

        // Update proposal stats
        let stats_key = DataKey::ProposalStats(circle_id);
        let mut stats: ProposalStats = env.storage().instance().get(&stats_key).unwrap_or(ProposalStats {
            total_proposals: 0,
            approved_proposals: 0,
            rejected_proposals: 0,
            executed_proposals: 0,
            average_participation: 0,
            average_voting_time: 0,
        });
        stats.total_proposals += 1;
        env.storage().instance().set(&stats_key, &stats);

        proposal_count
    }

    fn quadratic_vote(env: Env, voter: Address, proposal_id: u64, vote_weight: u32, vote_choice: QuadraticVoteChoice) {
        voter.require_auth();

        let proposal_key = DataKey::Proposal(proposal_id);
        let mut proposal: Proposal = env.storage().instance().get(&proposal_key)
            .expect("Proposal not found");

        if proposal.status != ProposalStatus::Active {
            panic!("Voting not active for this proposal");
        }

        let current_time = env.ledger().timestamp();
        if current_time > proposal.voting_end_timestamp {
            proposal.status = ProposalStatus::Expired;
            env.storage().instance().set(&proposal_key, &proposal);
            panic!("Voting period expired");
        }

        // Check if already voted
        let vote_key = DataKey::QuadraticVote(proposal_id, voter.clone());
        if env.storage().instance().has(&vote_key) {
            panic!("Already voted on this proposal");
        }

        // Get voting power
        let voting_power_key = DataKey::VotingPower(voter.clone(), proposal.circle_id);
        let voting_power: VotingPower = env.storage().instance().get(&voting_power_key)
            .expect("Voting power not calculated");

        if vote_weight > MAX_VOTE_WEIGHT {
            panic!("Vote weight exceeds maximum");
        }

        // Calculate quadratic voting cost: weight^2
        let voting_cost = (vote_weight as u64) * (vote_weight as u64);
        
        if voting_cost > voting_power.quadratic_power {
            panic!("Insufficient voting power");
        }

        // Record the vote
        let quadratic_vote = QuadraticVote {
            voter: voter.clone(),
            proposal_id,
            vote_weight,
            vote_choice,
            voting_power_used: voting_cost,
            timestamp: current_time,
        };

        env.storage().instance().set(&vote_key, &quadratic_vote);

        // Update proposal tallies
        match vote_choice {
            QuadraticVoteChoice::For => {
                proposal.for_votes += voting_cost;
            }
            QuadraticVoteChoice::Against => {
                proposal.against_votes += voting_cost;
            }
            QuadraticVoteChoice::Abstain => {
                // Abstain votes don't affect the outcome
            }
        }

        proposal.total_voting_power += voting_cost;

        // Check quorum
        let circle_key = DataKey::Circle(proposal.circle_id);
        let circle: CircleInfo = env.storage().instance().get(&circle_key).expect("Circle not found");
        let required_quorum = (circle.member_count * QUADRATIC_QUORUM) / 100;
        proposal.quorum_met = proposal.total_voting_power >= required_quorum as u64;

        env.storage().instance().set(&proposal_key, &proposal);
    }

    fn execute_proposal(env: Env, caller: Address, proposal_id: u64) {
        caller.require_auth();

        let proposal_key = DataKey::Proposal(proposal_id);
        let mut proposal: Proposal = env.storage().instance().get(&proposal_key)
            .expect("Proposal not found");

        if proposal.status != ProposalStatus::Active {
            panic!("Proposal not active");
        }

        let current_time = env.ledger().timestamp();
        if current_time <= proposal.voting_end_timestamp {
            panic!("Voting period not yet ended");
        }

        if !proposal.quorum_met {
            proposal.status = ProposalStatus::Rejected;
            env.storage().instance().set(&proposal_key, &proposal);
            panic!("Quorum not met");
        }

        // Calculate result
        let total_votes = proposal.for_votes + proposal.against_votes;
        if total_votes == 0 {
            proposal.status = ProposalStatus::Rejected;
        } else {
            let approval_percentage = (proposal.for_votes * 100) / total_votes;
            if approval_percentage >= QUADRATIC_MAJORITY as u64 {
                proposal.status = ProposalStatus::Approved;
                
                // Execute the proposal based on type
                self.execute_proposal_logic(&env, &proposal);
            } else {
                proposal.status = ProposalStatus::Rejected;
            }
        }

        env.storage().instance().set(&proposal_key, &proposal);

        // Update stats
        let stats_key = DataKey::ProposalStats(proposal.circle_id);
        let mut stats: ProposalStats = env.storage().instance().get(&stats_key).unwrap_or(ProposalStats {
            total_proposals: 0,
            approved_proposals: 0,
            rejected_proposals: 0,
            executed_proposals: 0,
            average_participation: 0,
            average_voting_time: 0,
        });

        match proposal.status {
            ProposalStatus::Approved => stats.approved_proposals += 1,
            ProposalStatus::Rejected => stats.rejected_proposals += 1,
            ProposalStatus::Executed => stats.executed_proposals += 1,
            _ => {}
        }

        env.storage().instance().set(&stats_key, &stats);
    }

    fn execute_proposal_logic(env: &Env, proposal: &Proposal) {
        // This would contain the logic to execute different proposal types
        // For now, we'll just mark as executed
        let proposal_key = DataKey::Proposal(proposal.id);
        let mut updated_proposal = proposal.clone();
        updated_proposal.status = ProposalStatus::Executed;
        env.storage().instance().set(&proposal_key, &updated_proposal);

        // In a full implementation, this would:
        // - Parse execution_data
        // - Execute the appropriate actions based on proposal_type
        // - Handle errors and rollbacks if needed
    }

    fn get_proposal(env: Env, proposal_id: u64) -> Proposal {
        let proposal_key = DataKey::Proposal(proposal_id);
        env.storage().instance().get(&proposal_key).expect("Proposal not found")
    }

    fn get_voting_power(env: Env, member: Address, circle_id: u64) -> VotingPower {
        let voting_power_key = DataKey::VotingPower(member, circle_id);
        env.storage().instance().get(&voting_power_key).unwrap_or(VotingPower {
            member,
            circle_id,
            token_balance: 0,
            quadratic_power: 0,
            last_updated: 0,
        })
    }

    fn get_proposal_stats(env: Env, circle_id: u64) -> ProposalStats {
        let stats_key = DataKey::ProposalStats(circle_id);
        env.storage().instance().get(&stats_key).unwrap_or(ProposalStats {
            total_proposals: 0,
            approved_proposals: 0,
            rejected_proposals: 0,
            executed_proposals: 0,
            average_participation: 0,
            average_voting_time: 0,
        })
    }

    fn update_voting_power(env: Env, member: Address, circle_id: u64, token_balance: i128) {
        // Calculate quadratic voting power as sqrt(token_balance)
        // We use integer approximation: sqrt(x) ≈ x / (sqrt(x) + 1) for simplicity
        // In production, you'd use a proper sqrt implementation
        
        let quadratic_power = if token_balance > 0 {
            // Simple approximation of square root for demonstration
            // In practice, you'd use a more accurate method
            let balance_u64 = token_balance as u64;
            (balance_u64 / 1000).max(1) // Simplified calculation
        } else {
            0
        };

        let voting_power = VotingPower {
            member: member.clone(),
            circle_id,
            token_balance,
            quadratic_power,
            last_updated: env.ledger().timestamp(),
        };

        env.storage().instance().set(&DataKey::VotingPower(member, circle_id), &voting_power);
    fn stake_collateral(env: Env, user: Address, circle_id: u64, amount: i128) {
        user.require_auth();
        
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        
        if !circle.requires_collateral {
            panic!("Collateral not required for this circle");
        }

        let collateral_key = DataKey::CollateralVault(user.clone(), circle_id);
        
        // Check if collateral already staked
        if let Some(_collateral) = env.storage().instance().get::<DataKey, CollateralInfo>(&collateral_key) {
            panic!("Collateral already staked");
        }

        // Calculate required collateral amount
        let required_collateral = (circle.total_cycle_value * circle.collateral_bps as i128) / 10000;
        
        if amount < required_collateral {
            panic!("Insufficient collateral amount");
        }

        // Transfer collateral to contract
        let token_client = token::Client::new(&env, &circle.token);
        token_client.transfer(&user, &env.current_contract_address(), &amount);

        // Create collateral record
        let collateral_info = CollateralInfo {
            member: user.clone(),
            circle_id,
            amount,
            status: CollateralStatus::Staked,
            staked_timestamp: env.ledger().timestamp(),
            release_timestamp: None,
        };

        env.storage().instance().set(&collateral_key, &collateral_info);
    }

    fn slash_collateral(env: Env, caller: Address, circle_id: u64, member: Address) {
        caller.require_auth();
        
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).expect("Not initialized");
        
        if caller != circle.creator && caller != stored_admin {
            panic!("Unauthorized");
        }

        let collateral_key = DataKey::CollateralVault(member.clone(), circle_id);
        let mut collateral_info: CollateralInfo = env.storage().instance().get(&collateral_key)
            .expect("Collateral not staked");

        if collateral_info.status != CollateralStatus::Staked {
            panic!("Collateral not available for slashing");
        }

        // Check if member is defaulted
        let defaulted_key = DataKey::DefaultedMembers(circle_id);
        let defaulted_members: Vec<Address> = env.storage().instance().get(&defaulted_key).unwrap_or(Vec::new(&env));
        
        if !defaulted_members.contains(&member) {
            panic!("Member not defaulted");
        }

        // Slash the collateral - distribute to remaining active members
        let token_client = token::Client::new(&env, &circle.token);
        let slash_amount = collateral_info.amount;
        
        // Get active members (excluding defaulted member)
        let mut active_members: Vec<Address> = Vec::new(&env);
        for i in 0..circle.max_members {
            // This is a simplified approach - in practice, you'd want to store member addresses more efficiently
            // For now, we'll distribute to group reserve
        }
        
        // Transfer to group reserve for distribution
        let mut reserve: i128 = env.storage().instance().get(&DataKey::GroupReserve).unwrap_or(0);
        reserve += slash_amount;
        env.storage().instance().set(&DataKey::GroupReserve, &reserve);

        // Update collateral status
        collateral_info.status = CollateralStatus::Slashed;
        env.storage().instance().set(&collateral_key, &collateral_info);
    }

    fn release_collateral(env: Env, caller: Address, circle_id: u64, member: Address) {
        caller.require_auth();
        
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).expect("Not initialized");
        
        if caller != circle.creator && caller != stored_admin && caller != member {
            panic!("Unauthorized");
        }

        let collateral_key = DataKey::CollateralVault(member.clone(), circle_id);
        let mut collateral_info: CollateralInfo = env.storage().instance().get(&collateral_key)
            .expect("Collateral not staked");

        if collateral_info.status != CollateralStatus::Staked {
            panic!("Collateral not available for release");
        }

        // Check if member has completed all contributions
        let member_key = DataKey::Member(member.clone());
        let member_info: Member = env.storage().instance().get(&member_key).expect("Member not found");
        
        if member_info.contribution_count < circle.max_members {
            panic!("Member has not completed all contributions");
        }

        // Release collateral back to member
        let token_client = token::Client::new(&env, &circle.token);
        token_client.transfer(&env.current_contract_address(), &member, &collateral_info.amount);

        // Update collateral status
        collateral_info.status = CollateralStatus::Released;
        collateral_info.release_timestamp = Some(env.ledger().timestamp());
        env.storage().instance().set(&collateral_key, &collateral_info);
    }

    fn mark_member_defaulted(env: Env, caller: Address, circle_id: u64, member: Address) {
        caller.require_auth();
        
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).expect("Not initialized");
        
        if caller != circle.creator && caller != stored_admin {
            panic!("Unauthorized");
        }

        let member_key = DataKey::Member(member.clone());
        let mut member_info: Member = env.storage().instance().get(&member_key).expect("Member not found");
        
        if member_info.status == MemberStatus::Defaulted {
            panic!("Member already defaulted");
        }

        // Mark member as defaulted
        member_info.status = MemberStatus::Defaulted;
        env.storage().instance().set(&member_key, &member_info);

        // Add to defaulted members list
        let defaulted_key = DataKey::DefaultedMembers(circle_id);
        let mut defaulted_members: Vec<Address> = env.storage().instance().get(&defaulted_key).unwrap_or(Vec::new(&env));
        
        if !defaulted_members.contains(&member) {
            defaulted_members.push_back(member.clone());
            env.storage().instance().set(&defaulted_key, &defaulted_members);
        }

        // Auto-slash collateral if staked
        let collateral_key = DataKey::CollateralVault(member.clone(), circle_id);
        if let Some(_collateral) = env.storage().instance().get::<DataKey, CollateralInfo>(&collateral_key) {
            // Reuse slash_collateral logic
            Self::slash_collateral(env, caller, circle_id, member);
        }
        
        // Check and slash vouch collateral if member was vouched for
        let vouch_key = DataKey::VouchCollateral(member.clone(), circle_id);
        if let Some(vouch_id) = env.storage().instance().get::<DataKey, u64>(&vouch_key) {
            // Find the voucher by checking all vouch records
            // In practice, you'd want a more efficient lookup, but this works for demonstration
            // We'll need to iterate through potential vouchers or store a reverse mapping
            // For now, we'll assume we can find the voucher and slash their collateral
            
            // This would require additional storage structure to efficiently find the voucher
            // For implementation, we'll add a reverse mapping
        }
    }
    
    // --- SOCIAL VOUCHING IMPLEMENTATION ---
    
    fn vouch_for_member(env: Env, voucher: Address, vouchee: Address, circle_id: u64, collateral_amount: i128) {
        voucher.require_auth();
        
        // Prevent self-vouching
        if voucher == vouchee {
            panic!("Cannot vouch for self");
        }
        
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        
        // Check if voucher is an active member
        let voucher_key = DataKey::Member(voucher.clone());
        let voucher_info: Member = env.storage().instance().get(&voucher_key).expect("Voucher not found");
        
        if voucher_info.status != MemberStatus::Active {
            panic!("Voucher not active");
        }
        
        // Check if vouchee is already a member
        let vouchee_key = DataKey::Member(vouchee.clone());
        if env.storage().instance().has(&vouchee_key) {
            panic!("Vouchee already member");
        }
        
        // Check voucher's trust score
        let social_capital_key = DataKey::SocialCapital(voucher.clone(), circle_id);
        let social_capital: SocialCapital = env.storage().instance().get(&social_capital_key).unwrap_or(SocialCapital {
            member: voucher.clone(),
            circle_id,
            leniency_given: 0,
            leniency_received: 0,
            voting_participation: 0,
            trust_score: 50,
            last_activity_timestamp: env.ledger().timestamp(),
            decay_count: 0,
        });
        
        if social_capital.trust_score < MIN_TRUST_SCORE_FOR_VOUCH {
            panic!("Insufficient trust score to vouch");
        }
        
        // Check if vouch already exists
        let vouch_record_key = DataKey::VouchRecord(voucher.clone(), vouchee.clone());
        if env.storage().instance().has(&vouch_record_key) {
            panic!("Vouch already exists");
        }
        
        // Check vouch limits
        let vouch_stats_key = DataKey::VouchStats(voucher.clone());
        let vouch_stats: VouchStats = env.storage().instance().get(&vouch_stats_key).unwrap_or(VouchStats {
            voucher: voucher.clone(),
            total_vouches_made: 0,
            active_vouches: 0,
            successful_vouches: 0,
            slashed_vouches: 0,
            total_collateral_locked: 0,
            total_collateral_lost: 0,
        });
        
        if vouch_stats.active_vouches >= MAX_VOUCHES_PER_MEMBER {
            panic!("Maximum active vouches exceeded");
        }
        
        // Calculate minimum required collateral
        let min_collateral = (circle.total_cycle_value * VOUCH_COLLATERAL_MULTIPLIER as i128) / 10000;
        if collateral_amount < min_collateral {
            panic!("Insufficient collateral amount");
        }
        
        // Transfer collateral from voucher to contract
        let token_client = token::Client::new(&env, &circle.token);
        token_client.transfer(&voucher, &env.current_contract_address(), &collateral_amount);
        
        let current_time = env.ledger().timestamp();
        let expiry_time = current_time + VOUCH_EXPIRY_SECONDS;
        
        // Create vouch record
        let vouch_record = VouchRecord {
            voucher: voucher.clone(),
            vouchee: vouchee.clone(),
            circle_id,
            collateral_amount,
            vouch_timestamp: current_time,
            expiry_timestamp: expiry_time,
            status: VouchStatus::Active,
            slash_count: 0,
        };
        
        env.storage().instance().set(&vouch_record_key, &vouch_record);
        
        // Store reverse mapping for efficient lookup
        let vouch_collateral_key = DataKey::VouchCollateral(vouchee.clone(), circle_id);
        env.storage().instance().set(&vouch_collateral_key, &circle_id); // Use circle_id as vouch_id for simplicity
        
        // Store reverse mapping to find voucher by vouchee
        let reverse_mapping_key = DataKey::VouchReverseMapping(vouchee.clone(), circle_id);
        env.storage().instance().set(&reverse_mapping_key, &voucher);
        
        // Update voucher stats
        let mut updated_stats = vouch_stats;
        updated_stats.total_vouches_made += 1;
        updated_stats.active_vouches += 1;
        updated_stats.total_collateral_locked += collateral_amount;
        env.storage().instance().set(&vouch_stats_key, &updated_stats);
        
        // Update voucher's social capital (vouching increases trust score)
        let mut updated_social_capital = social_capital;
        updated_social_capital.trust_score = (updated_social_capital.trust_score + 3).min(100);
        env.storage().instance().set(&social_capital_key, &updated_social_capital);
        
        // Update activity timestamp for reputation decay
        Self::update_activity_timestamp(&env, voucher, circle_id);
        
        // Check and award milestones based on vouching activity
        Self::check_and_award_milestones(env, voucher.clone(), circle_id);
        
        // Apply any pending milestone bonuses
        Self::apply_milestone_bonus(env, voucher.clone(), circle_id);
    }
    
    fn slash_vouch_collateral(env: Env, caller: Address, circle_id: u64, vouchee: Address) {
        caller.require_auth();
        
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).expect("Not initialized");
        
        if caller != circle.creator && caller != stored_admin {
            panic!("Unauthorized");
        }
        
        // Find the voucher using reverse mapping
        let reverse_mapping_key = DataKey::VouchReverseMapping(vouchee.clone(), circle_id);
        let voucher: Address = env.storage().instance().get(&reverse_mapping_key).expect("No vouch found");
        
        // Get the vouch record
        let vouch_record_key = DataKey::VouchRecord(voucher.clone(), vouchee.clone());
        let mut vouch_record: VouchRecord = env.storage().instance().get(&vouch_record_key).expect("Vouch record not found");
        
        if vouch_record.status != VouchStatus::Active {
            panic!("Vouch not active");
        }
        
        // Check if vouchee is defaulted
        let vouchee_key = DataKey::Member(vouchee.clone());
        if let Some(vouchee_info) = env.storage().instance().get::<DataKey, Member>(&vouchee_key) {
            if vouchee_info.status != MemberStatus::Defaulted {
                panic!("Vouchee not defaulted");
            }
        } else {
            panic!("Vouchee not found");
        }
        
        // Transfer collateral to group reserve
        let token_client = token::Client::new(&env, &circle.token);
        token_client.transfer(&env.current_contract_address(), &env.current_contract_address(), &vouch_record.collateral_amount);
        
        // Update group reserve
        let mut reserve: i128 = env.storage().instance().get(&DataKey::GroupReserve).unwrap_or(0);
        reserve += vouch_record.collateral_amount;
        env.storage().instance().set(&DataKey::GroupReserve, &reserve);
        
        // Update vouch record
        vouch_record.status = VouchStatus::Slashed;
        vouch_record.slash_count += 1;
        env.storage().instance().set(&vouch_record_key, &vouch_record);
        
        // Update voucher stats
        let vouch_stats_key = DataKey::VouchStats(voucher.clone());
        let mut vouch_stats: VouchStats = env.storage().instance().get(&vouch_stats_key).expect("Vouch stats not found");
        vouch_stats.active_vouches -= 1;
        vouch_stats.slashed_vouches += 1;
        vouch_stats.total_collateral_lost += vouch_record.collateral_amount;
        env.storage().instance().set(&vouch_stats_key, &vouch_stats);
        
        // Decrease voucher's trust score due to slash
        let social_capital_key = DataKey::SocialCapital(voucher.clone(), circle_id);
        let mut social_capital: SocialCapital = env.storage().instance().get(&social_capital_key).unwrap_or(SocialCapital {
            member: voucher.clone(),
            circle_id,
            leniency_given: 0,
            leniency_received: 0,
            voting_participation: 0,
            trust_score: 50,
            last_activity_timestamp: env.ledger().timestamp(),
            decay_count: 0,
        });
        social_capital.trust_score = (social_capital.trust_score - 10).max(0); // Significant penalty for slash
        env.storage().instance().set(&social_capital_key, &social_capital);
        
        // Clean up reverse mapping
        env.storage().instance().remove(&reverse_mapping_key);
        let vouch_collateral_key = DataKey::VouchCollateral(vouchee.clone(), circle_id);
        env.storage().instance().remove(&vouch_collateral_key);
    }
    
    fn release_vouch_collateral(env: Env, caller: Address, circle_id: u64, vouchee: Address) {
        caller.require_auth();
        
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).expect("Not initialized");
        
        if caller != circle.creator && caller != stored_admin {
            panic!("Unauthorized");
        }
        
        // Find the voucher using reverse mapping
        let reverse_mapping_key = DataKey::VouchReverseMapping(vouchee.clone(), circle_id);
        let voucher: Address = env.storage().instance().get(&reverse_mapping_key).expect("No vouch found");
        
        // Get the vouch record
        let vouch_record_key = DataKey::VouchRecord(voucher.clone(), vouchee.clone());
        let mut vouch_record: VouchRecord = env.storage().instance().get(&vouch_record_key).expect("Vouch record not found");
        
        if vouch_record.status != VouchStatus::Active {
            panic!("Vouch not active");
        }
        
        // Check if vouchee has completed all contributions
        let vouchee_key = DataKey::Member(vouchee.clone());
        let vouchee_info: Member = env.storage().instance().get(&vouchee_key).expect("Vouchee not found");
        
        if vouchee_info.contribution_count < circle.max_members {
            panic!("Vouchee has not completed all contributions");
        }
        
        // Return collateral to voucher
        let token_client = token::Client::new(&env, &circle.token);
        token_client.transfer(&env.current_contract_address(), &voucher, &vouch_record.collateral_amount);
        
        // Update vouch record
        vouch_record.status = VouchStatus::Completed;
        env.storage().instance().set(&vouch_record_key, &vouch_record);
        
        // Update voucher stats
        let vouch_stats_key = DataKey::VouchStats(voucher.clone());
        let mut vouch_stats: VouchStats = env.storage().instance().get(&vouch_stats_key).expect("Vouch stats not found");
        vouch_stats.active_vouches -= 1;
        vouch_stats.successful_vouches += 1;
        env.storage().instance().set(&vouch_stats_key, &vouch_stats);
        
        // Increase voucher's trust score due to successful vouch
        let social_capital_key = DataKey::SocialCapital(voucher.clone(), circle_id);
        let mut social_capital: SocialCapital = env.storage().instance().get(&social_capital_key).unwrap_or(SocialCapital {
            member: voucher.clone(),
            circle_id,
            leniency_given: 0,
            leniency_received: 0,
            voting_participation: 0,
            trust_score: 50,
            last_activity_timestamp: env.ledger().timestamp(),
            decay_count: 0,
        });
        social_capital.trust_score = (social_capital.trust_score + 5).min(100); // Bonus for successful vouch
        env.storage().instance().set(&social_capital_key, &social_capital);
        
        // Clean up reverse mapping
        env.storage().instance().remove(&reverse_mapping_key);
        let vouch_collateral_key = DataKey::VouchCollateral(vouchee.clone(), circle_id);
        env.storage().instance().remove(&vouch_collateral_key);
    }
    
    fn get_vouch_record(env: Env, voucher: Address, vouchee: Address) -> VouchRecord {
        let vouch_record_key = DataKey::VouchRecord(voucher, vouchee);
        env.storage().instance().get(&vouch_record_key).expect("Vouch record not found")
    }
    
    fn get_vouch_stats(env: Env, voucher: Address) -> VouchStats {
        let vouch_stats_key = DataKey::VouchStats(voucher);
        env.storage().instance().get(&vouch_stats_key).unwrap_or(VouchStats {
            voucher,
            total_vouches_made: 0,
            active_vouches: 0,
            successful_vouches: 0,
            slashed_vouches: 0,
            total_collateral_locked: 0,
            total_collateral_lost: 0,
        })
    }
    
    // --- REPUTATION DECAY IMPLEMENTATION ---
    
    fn apply_reputation_decay(env: Env, member: Address, circle_id: u64) {
        let current_time = env.ledger().timestamp();
        let social_capital_key = DataKey::SocialCapital(member.clone(), circle_id);
        let mut social_capital: SocialCapital = env.storage().instance().get(&social_capital_key).unwrap_or(SocialCapital {
            member: member.clone(),
            circle_id,
            leniency_given: 0,
            leniency_received: 0,
            voting_participation: 0,
            trust_score: 50,
            last_activity_timestamp: current_time,
            decay_count: 0,
        });
        
        // Check if member has been inactive for more than 18 months
        let months_inactive = if current_time > social_capital.last_activity_timestamp {
            (current_time - social_capital.last_activity_timestamp) / SECONDS_PER_MONTH
        } else {
            0
        };
        
        if months_inactive >= INACTIVITY_THRESHOLD_MONTHS {
            let months_to_decay = months_inactive - INACTIVITY_THRESHOLD_MONTHS + 1; // +1 to start decay immediately after threshold
            
            // Calculate total decay: 5% per month
            let total_decay_percentage = DECAY_PERCENTAGE_PER_MONTH * months_to_decay as u32;
            
            // Apply decay to trust score
            let decay_amount = (social_capital.trust_score * total_decay_percentage) / 10000;
            social_capital.trust_score = (social_capital.trust_score - decay_amount).max(0);
            social_capital.decay_count += months_to_decay as u32;
            
            // Store decay history
            let decay_history_key = DataKey::DecayHistory(member.clone(), circle_id);
            env.storage().instance().set(&decay_history_key, &current_time);
            
            // Update social capital
            env.storage().instance().set(&social_capital_key, &social_capital);
        }
    }
    
    fn update_activity_timestamp(env: Env, member: Address, circle_id: u64) {
        let current_time = env.ledger().timestamp();
        let social_capital_key = DataKey::SocialCapital(member.clone(), circle_id);
        let mut social_capital: SocialCapital = env.storage().instance().get(&social_capital_key).unwrap_or(SocialCapital {
            member: member.clone(),
            circle_id,
            leniency_given: 0,
            leniency_received: 0,
            voting_participation: 0,
            trust_score: 50,
            last_activity_timestamp: current_time,
            decay_count: 0,
        });
        
        // Update activity timestamp
        social_capital.last_activity_timestamp = current_time;
        
        // Store updated social capital
        env.storage().instance().set(&social_capital_key, &social_capital);
        
        // Also store global activity timestamp for easy lookup
        let activity_key = DataKey::LastActivityTimestamp(member);
        env.storage().instance().set(&activity_key, &current_time);
    }
    
    fn get_reputation_with_decay(env: Env, member: Address, circle_id: u64) -> SocialCapital {
        // Apply decay first, then return the updated social capital
        Self::apply_reputation_decay(env, member.clone(), circle_id);
        
        let social_capital_key = DataKey::SocialCapital(member, circle_id);
        env.storage().instance().get(&social_capital_key).unwrap_or(SocialCapital {
            member,
            circle_id,
            leniency_given: 0,
            leniency_received: 0,
            voting_participation: 0,
            trust_score: 50,
            last_activity_timestamp: env.ledger().timestamp(),
            decay_count: 0,
        })
    }
    
    // --- MILESTONE-BASED REPUTATION BOOST IMPLEMENTATION ---
    
    fn update_milestone_progress(env: Env, member: Address, circle_id: u64, milestone_type: MilestoneType, progress_increment: u32) {
        let current_time = env.ledger().timestamp();
        let progress_key = DataKey::MilestoneProgress(member.clone(), circle_id);
        
        // Get existing progress or create new
        let mut milestone_progress: MilestoneProgress = env.storage().instance().get(&progress_key).unwrap_or(MilestoneProgress {
            member: member.clone(),
            circle_id,
            milestone_type: milestone_type.clone(),
            current_progress: 0,
            target_progress: Self::get_milestone_target(&milestone_type),
            status: MilestoneStatus::InProgress,
            start_timestamp: current_time,
            completion_timestamp: None,
        });
        
        // Only update if milestone is still in progress
        if milestone_progress.status == MilestoneStatus::InProgress {
            milestone_progress.current_progress += progress_increment;
            
            // Check if milestone is completed
            if milestone_progress.current_progress >= milestone_progress.target_progress {
                milestone_progress.status = MilestoneStatus::Completed;
                milestone_progress.completion_timestamp = Some(current_time);
                
                // Award the milestone bonus
                Self::award_milestone_bonus(&env, &member, circle_id, &milestone_type);
                
                // Update milestone stats
                Self::update_milestone_stats(&env, circle_id, &milestone_type);
            }
            
            env.storage().instance().set(&progress_key, &milestone_progress);
        }
    }
    
    fn check_and_award_milestones(env: Env, member: Address, circle_id: u64) {
        // This function checks various conditions and awards milestones automatically
        // It's called after significant actions like deposits, voting, etc.
        
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).expect("Circle not found");
        let member_key = DataKey::Member(member.clone());
        let member_info: Member = env.storage().instance().get(&member_key).expect("Member not found");
        
        // Check consecutive on-time payments milestone
        if member_info.contribution_count >= 5 {
            Self::update_milestone_progress(env, member.clone(), circle_id, MilestoneType::ConsecutiveOnTimePayments, 1);
        }
        
        // Check perfect attendance (completed all contributions)
        if member_info.contribution_count >= circle.max_members {
            Self::update_milestone_progress(env, member.clone(), circle_id, MilestoneType::PerfectAttendance, 1);
        }
        
        // Check first group organized (for circle creators)
        if member_info.address == circle.creator {
            Self::update_milestone_progress(env, member.clone(), circle_id, MilestoneType::FirstGroupOrganized, 1);
        }
        
        // Check referral master milestone
        if let Some(referrer) = &member_info.referrer {
            Self::update_milestone_progress(env, referrer.clone(), circle_id, MilestoneType::ReferralMaster, 1);
        }
        
        // Check vouching champion milestone
        let vouch_stats_key = DataKey::VouchStats(member.clone());
        if let Some(vouch_stats) = env.storage().instance().get::<DataKey, VouchStats>(&vouch_stats_key) {
            if vouch_stats.successful_vouches >= 5 {
                Self::update_milestone_progress(env, member.clone(), circle_id, MilestoneType::VouchingChampion, 1);
            }
        }
        
        // Check community leader milestone (high voting participation)
        let social_capital_key = DataKey::SocialCapital(member.clone(), circle_id);
        if let Some(social_capital) = env.storage().instance().get::<DataKey, SocialCapital>(&social_capital_key) {
            if social_capital.voting_participation >= 10 {
                Self::update_milestone_progress(env, member.clone(), circle_id, MilestoneType::CommunityLeader, 1);
            }
        }
        
        // Check reliability star milestone (long-term consistency)
        if social_capital.trust_score >= 95 && social_capital.decay_count == 0 {
            Self::update_milestone_progress(env, member.clone(), circle_id, MilestoneType::ReliabilityStar, 1);
        }
    }
    
    fn apply_milestone_bonus(env: Env, member: Address, circle_id: u64) {
        let bonuses_key = DataKey::MilestoneBonuses(member.clone(), circle_id);
        let bonuses: Vec<MilestoneBonus> = env.storage().instance().get(&bonuses_key).unwrap_or(Vec::new(&env));
        
        let mut total_bonus = 0u32;
        let current_time = env.ledger().timestamp();
        
        // Calculate total unapplied bonus points
        for bonus in &bonuses {
            if !bonus.is_applied {
                total_bonus += bonus.bonus_points;
            }
        }
        
        if total_bonus > 0 {
            // Apply bonus to social capital
            let social_capital_key = DataKey::SocialCapital(member.clone(), circle_id);
            let mut social_capital: SocialCapital = env.storage().instance().get(&social_capital_key).unwrap_or(SocialCapital {
                member: member.clone(),
                circle_id,
                leniency_given: 0,
                leniency_received: 0,
                voting_participation: 0,
                trust_score: 50,
                last_activity_timestamp: current_time,
                decay_count: 0,
            });
            
            // Add bonus points to trust score (capped at 100)
            social_capital.trust_score = (social_capital.trust_score + total_bonus).min(100);
            env.storage().instance().set(&social_capital_key, &social_capital);
            
            // Mark bonuses as applied
            let mut updated_bonuses = bonuses;
            for bonus in &mut updated_bonuses {
                if !bonus.is_applied {
                    bonus.is_applied = true;
                }
            }
            env.storage().instance().set(&bonuses_key, &updated_bonuses);
        }
    }
    
    fn get_milestone_progress(env: Env, member: Address, circle_id: u64, milestone_type: MilestoneType) -> MilestoneProgress {
        let progress_key = DataKey::MilestoneProgress(member, circle_id);
        env.storage().instance().get(&progress_key).unwrap_or(MilestoneProgress {
            member,
            circle_id,
            milestone_type,
            current_progress: 0,
            target_progress: Self::get_milestone_target(&milestone_type),
            status: MilestoneStatus::InProgress,
            start_timestamp: env.ledger().timestamp(),
            completion_timestamp: None,
        })
    }
    
    fn get_milestone_bonuses(env: Env, member: Address, circle_id: u64) -> Vec<MilestoneBonus> {
        let bonuses_key = DataKey::MilestoneBonuses(member, circle_id);
        env.storage().instance().get(&bonuses_key).unwrap_or(Vec::new(&env))
    }
    
    fn get_milestone_stats(env: Env, circle_id: u64) -> MilestoneStats {
        let stats_key = DataKey::MilestoneStats(circle_id);
        env.storage().instance().get(&stats_key).unwrap_or(MilestoneStats {
            circle_id,
            total_milestones_completed: 0,
            total_bonus_points_distributed: 0,
            members_with_milestones: 0,
            most_common_milestone: MilestoneType::ConsecutiveOnTimePayments,
        })
    }
    
    fn calculate_total_reputation_boost(env: Env, member: Address, circle_id: u64) -> u32 {
        let bonuses_key = DataKey::MilestoneBonuses(member, circle_id);
        let bonuses: Vec<MilestoneBonus> = env.storage().instance().get(&bonuses_key).unwrap_or(Vec::new(&env));
        
        let mut total_boost = 0u32;
        for bonus in &bonuses {
            if bonus.is_applied {
                total_boost += bonus.bonus_points;
            }
        }
        
        total_boost
    }
    
    // --- HELPER FUNCTIONS FOR MILESTONE SYSTEM ---
    
    fn get_milestone_target(milestone_type: &MilestoneType) -> u32 {
        match milestone_type {
            MilestoneType::ConsecutiveOnTimePayments => 5, // Base target, increases with tiers
            MilestoneType::FirstGroupOrganized => 1,
            MilestoneType::PerfectAttendance => 1,
            MilestoneType::EarlyBirdStreak => 3,
            MilestoneType::ReferralMaster => 3,
            MilestoneType::VouchingChampion => 5,
            MilestoneType::CommunityLeader => 10,
            MilestoneType::ReliabilityStar => 1,
        }
    }
    
    fn get_milestone_bonus_points(milestone_type: &MilestoneType, progress: u32) -> u32 {
        match milestone_type {
            MilestoneType::ConsecutiveOnTimePayments => {
                match progress {
                    5 => CONSECUTIVE_ON_TIME_BONUS_5,
                    10 => CONSECUTIVE_ON_TIME_BONUS_10,
                    12 => CONSECUTIVE_ON_TIME_BONUS_12,
                    _ => 0,
                }
            }
            MilestoneType::FirstGroupOrganized => FIRST_GROUP_ORGANIZED_BONUS,
            MilestoneType::PerfectAttendance => PERFECT_ATTENDANCE_BONUS,
            MilestoneType::EarlyBirdStreak => EARLY_BIRD_STREAK_BONUS,
            MilestoneType::ReferralMaster => REFERRAL_MASTER_BONUS,
            MilestoneType::VouchingChampion => VOUCHING_CHAMPION_BONUS,
            MilestoneType::CommunityLeader => COMMUNITY_LEADER_BONUS,
            MilestoneType::ReliabilityStar => RELIABILITY_STAR_BONUS,
        }
    }
    
    fn award_milestone_bonus(env: &Env, member: &Address, circle_id: u64, milestone_type: &MilestoneType) {
        let current_time = env.ledger().timestamp();
        let bonus_points = Self::get_milestone_bonus_points(milestone_type, 1); // Base completion
        
        if bonus_points > 0 {
            let bonuses_key = DataKey::MilestoneBonuses(member.clone(), circle_id);
            let mut bonuses: Vec<MilestoneBonus> = env.storage().instance().get(&bonuses_key).unwrap_or(Vec::new(env));
            
            let new_bonus = MilestoneBonus {
                member: member.clone(),
                circle_id,
                milestone_type: milestone_type.clone(),
                bonus_points,
                earned_timestamp: current_time,
                is_applied: false,
            };
            
            bonuses.push_back(new_bonus);
            env.storage().instance().set(&bonuses_key, &bonuses);
        }
    }
    
    fn update_milestone_stats(env: &Env, circle_id: u64, milestone_type: &MilestoneType) {
        let stats_key = DataKey::MilestoneStats(circle_id);
        let mut stats: MilestoneStats = env.storage().instance().get(&stats_key).unwrap_or(MilestoneStats {
            circle_id,
            total_milestones_completed: 0,
            total_bonus_points_distributed: 0,
            members_with_milestones: 0,
            most_common_milestone: milestone_type.clone(),
        });
        
        stats.total_milestones_completed += 1;
        stats.total_bonus_points_distributed += Self::get_milestone_bonus_points(milestone_type, 1);
        stats.most_common_milestone = milestone_type.clone();
        
        env.storage().instance().set(&stats_key, &stats);
    }
}
