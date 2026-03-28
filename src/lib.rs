#![no_std]
use soroban_sdk::{contract, contracttype, contractimpl, contractclient, Address, Env, Vec, Symbol, token, testutils::{Address as TestAddress, Arbitrary as TestArbitrary}, arbitrary::{Arbitrary, Unstructured}, Map};

// --- ERROR CODES ---
pub const CLAWBACK_DETECTED: u32 = 2001;
pub const ROUND_ALREADY_PAUSED: u32 = 2002;
pub const ROUND_NOT_PAUSED: u32 = 2003;
pub const INSUFFICIENT_RECOVERY_FUNDS: u32 = 2004;
pub const RECOVERY_PLAN_NOT_ACTIVE: u32 = 2005;

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
    // Governance Token Mining Keys
    GovernanceToken,
    VestingVault,
    UserVesting(Address),
    MiningConfig,
    TotalMinedTokens,
    UserMiningStats(Address),
    // Clawback Reconciliation Keys
    ClawbackDeficit(u64), // circle_id -> deficit amount
    RecoveryPlan(u64),    // circle_id -> recovery plan
    PausedRounds(u64),    // circle_id -> pause info
    // Group Lead Commission Keys
    MemberByIndex(u64, u32), // circle_id -> member_index -> member_address
    // Soulbound Token (SBT) Keys
    SbtContract,
    UserSbt(Address), // user -> SBT info
    ReputationMilestone(u32), // milestone_id -> milestone config
    SbtRevocationList(Address), // user -> revocation info
    // Stellar Anchor Interface Keys
    AnchorRegistry, // registered anchors
    AnchorInfo(Address), // anchor_address -> anchor info
    AnchorConfig(Address), // anchor_address -> deposit config
    PendingDeposit(u64), // deposit_id -> deposit info
}

#[contracttype]
#[derive(Clone)]
pub struct Member {
    pub address: Address,
    pub index: u32,
    pub contribution_count: u32,
    pub last_contribution_time: u64,
    pub is_active: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct CircleInfo {
    pub id: u64,
    pub creator: Address,
    pub contribution_amount: u64,
    pub max_members: u16,
    pub member_count: u16,
    pub current_recipient_index: u16,
    pub is_active: bool,
    pub token: Address,
    pub deadline_timestamp: u64,
    pub cycle_duration: u64,
    pub contribution_bitmap: u64,
    pub payout_bitmap: u64,
    pub insurance_balance: u64,
    pub insurance_fee_bps: u32,
    pub is_insurance_used: bool,
    pub late_fee_bps: u32,
    pub proposed_late_fee_bps: u32,
    pub proposal_votes_bitmap: u64,
    pub nft_contract: Address,
    pub cycle_count: u32, // Track completed cycles for vesting
    pub is_paused: bool, // Pause state for clawback events
    pub expected_balance: u64, // Expected token balance for deficit detection
    pub organizer_fee_bps: u32, // Commission for group creator in basis points (1% = 100 bps)
    // Business Goal Verification (Issue #212)
    business_goal_hash: Option<Symbol>, // Hash of business goal document
    verified_vendor: Option<Address>, // Verified vendor for goal verification
    goal_amount: Option<u64>, // Amount needed for business goal
    pub is_goal_verified: bool, // Whether goal has been verified
}

// --- STELLAR ANCHOR INTERFACE STRUCTURES ---

#[contracttype]
#[derive(Clone, Debug)]
pub enum AnchorStatus {
    Active,
    Suspended,
    Revoked,
}

#[contracttype]
#[derive(Clone)]
pub struct AnchorInfo {
    pub address: Address,
    pub name: Symbol,
    pub sep_version: Symbol, // "SEP-24" or "SEP-31"
    pub status: AnchorStatus,
    pub registration_timestamp: u64,
    pub kyc_required: bool,
    pub supported_tokens: Vec<Address>,
    pub max_deposit_amount: u64,
    pub daily_deposit_limit: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct AnchorDepositConfig {
    pub anchor_address: Address,
    pub circle_id: u64,
    pub auto_deposit_enabled: bool,
    pub gas_subsidy_amount: u64,
    pub fee_bps: u32,
    pub last_deposit_timestamp: u64,
    pub total_deposits_made: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub enum DepositStatus {
    Pending,
    Completed,
    Failed,
    Refunded,
}

#[contracttype]
#[derive(Clone)]
pub struct AnchorDeposit {
    pub deposit_id: u64,
    pub anchor_address: Address,
    pub beneficiary: Address,
    pub circle_id: u64,
    pub amount: u64,
    pub token_address: Address,
    pub fiat_reference: Symbol, // External transaction reference
    pub status: DepositStatus,
    pub timestamp: u64,
    pub gas_used: u64,
    pub error_message: Option<Symbol>,
}

// --- SOULBOUND TOKEN (SBT) STRUCTURES ---

#[contracttype]
#[derive(Clone, Debug)]
pub enum SbtStatus {
    Active,
    Dishonored,
    Revoked,
}

#[contracttype]
#[derive(Clone)]
pub struct SoulboundToken {
    pub token_id: u128,
    pub owner: Address,
    pub milestone_id: u32,
    pub issued_at: u64,
    pub status: SbtStatus,
    pub reputation_score: u32,
    pub cycles_completed: u32,
    metadata: Symbol,
}

#[contracttype]
#[derive(Clone)]
pub struct ReputationMilestone {
    pub id: u32,
    pub name: Symbol,
    pub description: Symbol,
    pub required_cycles: u32,
    pub min_reputation_score: u32,
    is_active: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct SbtRevocationInfo {
    pub user: Address,
    pub token_id: u128,
    pub revoked_at: u64,
    pub reason: Symbol,
    pub revoked_by: Address,
}

#[contracttype]
#[derive(Clone)]
pub struct MiningConfig {
    pub tokens_per_contribution: u64,
    pub vesting_duration_cycles: u32,
    pub cliff_cycles: u32,
    pub max_mining_per_circle: u64,
    pub is_mining_enabled: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct UserVestingInfo {
    pub total_allocated: u64,
    pub vested_amount: u64,
    pub claimed_amount: u64,
    pub start_cycle: u32,
    pub contributions_made: u32,
    pub is_active: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct UserMiningStats {
    pub total_contributions: u32,
    pub total_tokens_earned: u64,
    pub total_tokens_claimed: u64,
    pub join_timestamp: u64,
    pub last_mining_timestamp: u64,
}

// --- CLAWBACK RECONCILIATION STRUCTURES ---

#[contracttype]
#[derive(Clone)]
pub struct ClawbackDeficit {
    pub circle_id: u64,
    pub deficit_amount: u64,
    pub detection_timestamp: u64,
    pub detected_by: Address,
    pub token_address: Address,
}

#[contracttype]
#[derive(Clone)]
pub struct RecoveryPlan {
    pub circle_id: u64,
    pub total_deficit: u64,
    pub recovery_type: RecoveryType,
    pub proposed_by: Address,
    pub proposal_timestamp: u64,
    pub votes_for: u16,
    pub votes_against: u16,
    pub is_active: bool,
    pub recovery_contributions: Map<Address, u64>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub enum RecoveryType {
    MemberContribution, // Members chip in extra funds
    PayoutReduction,    // Next payout is reduced
    InsuranceUsage,     // Use insurance funds
    Hybrid,             // Combination of approaches
}

#[contracttype]
#[derive(Clone)]
pub struct PausedRound {
    pub circle_id: u64,
    pub pause_timestamp: u64,
    pub pause_reason: PauseReason,
    pub paused_by: Address,
}

#[contracttype]
#[derive(Clone, Debug)]
pub enum PauseReason {
    ClawbackDetected,
    DeficitReconciliation,
    EmergencyMaintenance,
}

// --- CONTRACT TRAITS ---

pub trait SoroSusuTrait {
    fn init(env: Env, admin: Address);
    fn create_circle(env: Env, creator: Address, amount: u64, max_members: u16, token: Address, cycle_duration: u64, insurance_fee_bps: u32, nft_contract: Address, organizer_fee_bps: u32) -> u64;
    fn join_circle(env: Env, user: Address, circle_id: u64);
    fn deposit(env: Env, user: Address, circle_id: u64);
    fn distribute_payout(env: Env, caller: Address, circle_id: u64);
    fn trigger_insurance_coverage(env: Env, caller: Address, circle_id: u64, member: Address);
    fn propose_penalty_change(env: Env, user: Address, circle_id: u64, new_bps: u32);
    fn vote_penalty_change(env: Env, user: Address, circle_id: u64);
    fn eject_member(env: Env, caller: Address, circle_id: u64, member: Address);
    
    // Governance Token Mining Functions
    fn set_governance_token(env: Env, admin: Address, token_address: Address);
    fn configure_mining(env: Env, admin: Address, config: MiningConfig);
    fn claim_vested_tokens(env: Env, user: Address);
    fn get_user_vesting_info(env: Env, user: Address) -> UserVestingInfo;
    fn get_mining_stats(env: Env, user: Address) -> UserMiningStats;
    fn complete_circle_cycle(env: Env, circle_id: u64);
    
    // Clawback Reconciliation Functions
    fn detect_and_handle_clawback(env: Env, caller: Address, circle_id: u64);
    fn pause_round(env: Env, caller: Address, circle_id: u64, reason: PauseReason);
    fn propose_recovery_plan(env: Env, caller: Address, circle_id: u64, recovery_type: RecoveryType);
    fn vote_recovery_plan(env: Env, caller: Address, circle_id: u64, vote_for: bool);
    fn contribute_to_recovery(env: Env, caller: Address, circle_id: u64, amount: u64);
    fn execute_recovery_plan(env: Env, caller: Address, circle_id: u64);
    fn resume_round(env: Env, caller: Address, circle_id: u64);
    fn get_clawback_deficit(env: Env, circle_id: u64) -> ClawbackDeficit;
    fn get_recovery_plan(env: Env, circle_id: u64) -> RecoveryPlan;
    fn get_paused_round_info(env: Env, circle_id: u64) -> PausedRound;
    
    // Stellar Anchor Interface Functions (SEP-24/SEP-31)
    fn register_anchor(env: Env, admin: Address, anchor_address: Address, name: Symbol, sep_version: Symbol, kyc_required: bool, supported_tokens: Vec<Address>, max_deposit_amount: u64, daily_deposit_limit: u64);
    fn deposit_for_user(env: Env, anchor: Address, beneficiary: Address, circle_id: u64, amount: u64, token_address: Address, fiat_reference: Symbol);
    fn configure_anchor_deposit(env: Env, anchor: Address, circle_id: u64, auto_deposit_enabled: bool, gas_subsidy_amount: u64, fee_bps: u32);
    fn get_anchor_info(env: Env, anchor_address: Address) -> AnchorInfo;
    fn get_deposit_status(env: Env, deposit_id: u64) -> AnchorDeposit;
    fn get_registered_anchors(env: Env) -> Vec<Address>;
    
    // SBT Credential Functions
    fn initialize_sbt_system(env: Env, admin: Address, sbt_contract: Address);
    fn create_reputation_milestone(env: Env, admin: Address, milestone_id: u32, name: Symbol, description: Symbol, required_cycles: u32, min_reputation_score: u32);
    fn issue_sbt_credential(env: Env, admin: Address, user: Address, milestone_id: u32);
    fn revoke_sbt_credential(env: Env, admin: Address, user: Address, reason: Symbol);
    fn update_sbt_status(env: Env, admin: Address, user: Address, status: SbtStatus);
    fn get_user_sbt(env: Env, user: Address) -> Option<SoulboundToken>;
    fn get_reputation_milestone(env: Env, milestone_id: u32) -> ReputationMilestone;
    fn verify_user_reputation(env: Env, user: Address) -> (u32, bool); // (score, has_sbt)
    fn get_paused_round_info(env: Env, circle_id: u64) -> PausedRound;
    
    // Soulbound Token (SBT) Functions (Issue #210)
    fn set_sbt_contract(env: Env, admin: Address, sbt_contract: Address);
    fn configure_reputation_milestone(env: Env, admin: Address, milestone_id: u32, milestone: ReputationMilestone);
    fn issue_sbt_credential(env: Env, user: Address, milestone_id: u32);
    fn revoke_sbt_credential(env: Env, admin: Address, user: Address, reason: Symbol);
    fn update_sbt_status(env: Env, admin: Address, user: Address, status: SbtStatus);
    fn get_user_sbt(env: Env, user: Address) -> SoulboundToken;
    fn get_reputation_milestone(env: Env, milestone_id: u32) -> ReputationMilestone;
    
    // Business Goal Verification Functions (Issue #212)
    fn set_business_goal(env: Env, creator: Address, circle_id: u64, goal_hash: Symbol, verified_vendor: Address, goal_amount: u64);
    fn verify_business_goal(env: Env, vendor: Address, circle_id: u64, invoice_hash: Symbol);
    fn release_goal_funds(env: Env, circle_id: u64);
    fn get_business_goal_info(env: Env, circle_id: u64) -> (Option<Symbol>, Option<Address>, Option<u64>, bool);
}

#[contractclient(name = "SusuNftClient")]
pub trait SusuNftTrait {
    fn mint(env: Env, to: Address, token_id: u128);
    fn burn(env: Env, from: Address, token_id: u128);
}

#[contractclient(name = "GovernanceTokenClient")]
pub trait GovernanceTokenTrait {
    fn mint(env: Env, to: Address, amount: u64);
}

#[contractclient(name = "SbtTokenClient")]
pub trait SbtTokenTrait {
    fn mint_sbt(env: Env, to: Address, token_id: u128, metadata: Symbol);
    fn update_metadata(env: Env, token_id: u128, metadata: Symbol);
    fn burn(env: Env, token_id: u128);
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
        
        // Initialize governance mining with default config
        let default_config = MiningConfig {
            tokens_per_contribution: 100, // 100 tokens per contribution
            vesting_duration_cycles: 12,   // 12 cycles (1 year if monthly)
            cliff_cycles: 3,               // 3 cycles cliff period
            max_mining_per_circle: 1000,    // Max 1000 tokens per circle
            is_mining_enabled: false,       // Disabled by default
        };
        env.storage().instance().set(&DataKey::MiningConfig, &default_config);
        env.storage().instance().set(&DataKey::TotalMinedTokens, &0u64);
    }

    fn create_circle(env: Env, creator: Address, amount: u64, max_members: u16, token: Address, cycle_duration: u64, insurance_fee_bps: u32, nft_contract: Address, organizer_fee_bps: u32) -> u64 {
        let mut circle_count: u64 = env.storage().instance().get(&DataKey::CircleCount).unwrap_or(0);
        circle_count += 1;

        if max_members > 64 {
            panic!("Max members cannot exceed 64 for optimization");
        }

        if insurance_fee_bps > 10000 {
            panic!("Insurance fee cannot exceed 100%");
        }

        if organizer_fee_bps > 10000 {
            panic!("Organizer fee cannot exceed 100%");
        }

        let current_time = env.ledger().timestamp();
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
            payout_bitmap: 0,
            insurance_balance: 0,
            insurance_fee_bps,
            is_insurance_used: false,
            late_fee_bps: 100,
            proposed_late_fee_bps: 0,
            proposal_votes_bitmap: 0,
            nft_contract,
            cycle_count: 0,
            is_paused: false,
            expected_balance: 0,
            organizer_fee_bps,
        };

        env.storage().instance().set(&DataKey::Circle(circle_count), &new_circle);
        env.storage().instance().set(&DataKey::CircleCount, &circle_count);

        if !env.storage().instance().has(&DataKey::GroupReserve) {
            env.storage().instance().set(&DataKey::GroupReserve, &0u64);
        }

        circle_count
    }

    fn join_circle(env: Env, user: Address, circle_id: u64) {
        user.require_auth();

        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();

        if circle.member_count >= circle.max_members {
            panic!("Circle is full");
        }

        let member_key = DataKey::Member(user.clone());
        if env.storage().instance().has(&member_key) {
            panic!("User is already a member");
        }

        let new_member = Member {
            address: user.clone(),
            index: circle.member_count as u32,
            contribution_count: 0,
            last_contribution_time: 0,
            is_active: true,
        };
        
        env.storage().instance().set(&member_key, &new_member);
        
        // Store member address by index for efficient lookup
        let member_by_index_key = DataKey::MemberByIndex(circle_id, new_member.index);
        env.storage().instance().set(&member_by_index_key, &user);
        
        circle.member_count += 1;
        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);

        // Initialize user mining stats
        let stats_key = DataKey::UserMiningStats(user.clone());
        if !env.storage().instance().has(&stats_key) {
            let stats = UserMiningStats {
                total_contributions: 0,
                total_tokens_earned: 0,
                total_tokens_claimed: 0,
                join_timestamp: env.ledger().timestamp(),
                last_mining_timestamp: 0,
            };
            env.storage().instance().set(&stats_key, &stats);
        }

        // Initialize user vesting info
        let vesting_key = DataKey::UserVesting(user.clone());
        if !env.storage().instance().has(&vesting_key) {
            let vesting_info = UserVestingInfo {
                total_allocated: 0,
                vested_amount: 0,
                claimed_amount: 0,
                start_cycle: 0,
                contributions_made: 0,
                is_active: false,
            };
            env.storage().instance().set(&vesting_key, &vesting_info);
        }

        let token_id = (circle_id as u128) << 64 | (new_member.index as u128);
        let client = SusuNftClient::new(&env, &circle.nft_contract);
        client.mint(&user, &token_id);
    }

    fn deposit(env: Env, user: Address, circle_id: u64) {
        user.require_auth();

        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();

        // Check if round is paused
        if circle.is_paused {
            panic!("Round is paused due to clawback detection");
        }

        let member_key = DataKey::Member(user.clone());
        let mut member: Member = env.storage().instance().get(&member_key)
            .unwrap_or_else(|| panic!("User is not a member of this circle"));

        if !member.is_active {
            panic!("Member is ejected");
        }

        let client = token::Client::new(&env, &circle.token);

        let current_time = env.ledger().timestamp();
        let mut penalty_amount = 0u64;

        if current_time > circle.deadline_timestamp {
            penalty_amount = (circle.contribution_amount * circle.late_fee_bps as u64) / 10000;
            
            let mut reserve_balance: u64 = env.storage().instance().get(&DataKey::GroupReserve).unwrap_or(0);
            reserve_balance += penalty_amount;
            env.storage().instance().set(&DataKey::GroupReserve, &reserve_balance);
        }

        let insurance_fee = ((circle.contribution_amount as u128 * circle.insurance_fee_bps as u128) / 10000) as u64;
        let total_amount = circle.contribution_amount + insurance_fee;

        // Update expected balance before transfer
        circle.expected_balance += total_amount;

        client.transfer(&user, &env.current_contract_address(), &total_amount);

        if insurance_fee > 0 {
            circle.insurance_balance += insurance_fee;
        }

        // ** GOVERNANCE TOKEN MINING LOGIC **
        Self::mine_governance_tokens(env.clone(), user.clone(), circle_id, &mut circle, &mut member);

        member.contribution_count += 1;
        member.last_contribution_time = current_time;
        
        env.storage().instance().set(&member_key, &member);

        circle.deadline_timestamp = current_time + circle.cycle_duration;
        circle.contribution_bitmap |= 1 << member.index;
        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);

        // Check if cycle is complete and trigger payout/mining release
        Self::check_and_complete_cycle(env.clone(), circle_id);

        // Check if user qualifies for SBT credential after this contribution
        Self::check_and_issue_sbt_credential(env.clone(), user.clone());
    }

    fn distribute_payout(env: Env, caller: Address, circle_id: u64) {
        caller.require_auth();

        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();
        
        // Check if round is paused
        if circle.is_paused {
            panic!("Round is paused due to clawback detection");
        }

        // Check if all members have contributed for this cycle
        let required_contributions = circle.member_count;
        let current_contributions = circle.contribution_bitmap.count_ones() as u16;
        
        if current_contributions < required_contributions {
            panic!("Not all members have contributed this cycle");
        }

        // Check if payout has already been processed for this cycle
        if (circle.payout_bitmap & (1 << circle.current_recipient_index)) != 0 {
            panic!("Payout already processed for this recipient this cycle");
        }

        // Calculate payout amount
        let base_payout_amount = circle.contribution_amount * circle.member_count as u64;
        let commission_amount = (base_payout_amount * circle.organizer_fee_bps as u64) / 10000;
        let net_payout_amount = base_payout_amount - commission_amount;

        let token_client = token::Client::new(&env, &circle.token);

        // Transfer commission to organizer if applicable
        if commission_amount > 0 {
            token_client.transfer(&env.current_contract_address(), &circle.creator, &commission_amount);
            
            // Emit commission event
            env.events().publish(
                (Symbol::short("commission_paid"), circle_id, circle.creator.clone()),
                commission_amount,
            );
        }

        // Find the current recipient
        let recipient_address = Self::get_current_recipient(env.clone(), circle_id);
        
        // Transfer net payout to recipient
        token_client.transfer(&env.current_contract_address(), &recipient_address, &net_payout_amount);

        // Mark payout as processed
        circle.payout_bitmap |= 1 << circle.current_recipient_index;
        
        // Move to next recipient
        circle.current_recipient_index = (circle.current_recipient_index + 1) % circle.member_count;
        
        // If we've completed a full round, reset payout bitmap and increment cycle
        if circle.current_recipient_index == 0 {
            circle.payout_bitmap = 0;
            circle.cycle_count += 1;
            circle.contribution_bitmap = 0; // Reset for next cycle
            circle.is_insurance_used = false;
            circle.deadline_timestamp = env.ledger().timestamp() + circle.cycle_duration;
        }

        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);

        // Emit payout event
        env.events().publish(
            (Symbol::short("payout_distributed"), circle_id, recipient_address),
            net_payout_amount,
        );
    }

    fn set_governance_token(env: Env, admin: Address, token_address: Address) {
        // Check admin authorization
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .unwrap_or_else(|| panic!("Admin not set"));
        if admin != stored_admin {
            panic!("Unauthorized: Only admin can set governance token");
        }

        env.storage().instance().set(&DataKey::GovernanceToken, &token_address);
        
        // Enable mining by default when token is set
        let mut config: MiningConfig = env.storage().instance().get(&DataKey::MiningConfig).unwrap();
        config.is_mining_enabled = true;
        env.storage().instance().set(&DataKey::MiningConfig, &config);
    }

    fn configure_mining(env: Env, admin: Address, config: MiningConfig) {
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .unwrap_or_else(|| panic!("Admin not set"));
        if admin != stored_admin {
            panic!("Unauthorized: Only admin can configure mining");
        }

        if config.tokens_per_contribution == 0 {
            panic!("Tokens per contribution must be greater than 0");
        }

        if config.vesting_duration_cycles == 0 {
            panic!("Vesting duration must be greater than 0");
        }

        if config.cliff_cycles > config.vesting_duration_cycles {
            panic!("Cliff period cannot exceed vesting duration");
        }

        env.storage().instance().set(&DataKey::MiningConfig, &config);
    }

    fn claim_vested_tokens(env: Env, user: Address) {
        user.require_auth();

        let governance_token: Address = env.storage().instance().get(&DataKey::GovernanceToken)
            .unwrap_or_else(|| panic!("Governance token not set"));

        let vesting_key = DataKey::UserVesting(user.clone());
        let mut vesting_info: UserVestingInfo = env.storage().instance().get(&vesting_key)
            .unwrap_or_else(|| panic!("No vesting info found for user"));

        if !vesting_info.is_active || vesting_info.total_allocated == 0 {
            panic!("No active vesting found");
        }

        let current_cycle = Self::get_current_global_cycle(env.clone());
        let vested_amount = Self::calculate_vested_amount(
            vesting_info.total_allocated,
            vesting_info.start_cycle,
            current_cycle,
            vesting_info.contributions_made,
        );

        let claimable_amount = vested_amount - vesting_info.claimed_amount;
        if claimable_amount == 0 {
            panic!("No tokens available to claim");
        }

        // Update claimed amount
        vesting_info.claimed_amount += claimable_amount;
        env.storage().instance().set(&vesting_key, &vesting_info);

        // Update user stats
        let stats_key = DataKey::UserMiningStats(user.clone());
        let mut stats: UserMiningStats = env.storage().instance().get(&stats_key).unwrap();
        stats.total_tokens_claimed += claimable_amount;
        env.storage().instance().set(&stats_key, &stats);

        // Transfer tokens
        let token_client = token::Client::new(&env, &governance_token);
        token_client.transfer(&env.current_contract_address(), &user, &claimable_amount);

        // Emit event
        env.events().publish(
            (Symbol::short("tokens_claimed"), user.clone()),
            claimable_amount,
        );
    }

    fn get_user_vesting_info(env: Env, user: Address) -> UserVestingInfo {
        let vesting_key = DataKey::UserVesting(user);
        env.storage().instance().get(&vesting_key)
            .unwrap_or_else(|| UserVestingInfo {
                total_allocated: 0,
                vested_amount: 0,
                claimed_amount: 0,
                start_cycle: 0,
                contributions_made: 0,
                is_active: false,
            })
    }

    fn get_mining_stats(env: Env, user: Address) -> UserMiningStats {
        let stats_key = DataKey::UserMiningStats(user);
        env.storage().instance().get(&stats_key)
            .unwrap_or_else(|| UserMiningStats {
                total_contributions: 0,
                total_tokens_earned: 0,
                total_tokens_claimed: 0,
                join_timestamp: 0,
                last_mining_timestamp: 0,
            })
    }

    fn complete_circle_cycle(env: Env, circle_id: u64) {
        Self::check_and_complete_cycle(env, circle_id);
    }

    fn trigger_insurance_coverage(env: Env, caller: Address, circle_id: u64, member: Address) {
        caller.require_auth();

        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();

        if caller != circle.creator {
            panic!("Unauthorized: Only creator can trigger insurance");
        }

        if circle.is_insurance_used {
            panic!("Insurance already used this cycle");
        }

        if circle.insurance_balance < circle.contribution_amount {
            panic!("Insufficient insurance balance");
        }

        let member_key = DataKey::Member(member.clone());
        let member_info: Member = env.storage().instance().get(&member_key).unwrap();

        if !member_info.is_active {
            panic!("Member is ejected");
        }

        if (circle.contribution_bitmap & (1 << member_info.index)) != 0 {
            panic!("Member already contributed");
        }

        circle.contribution_bitmap |= 1 << member_info.index;
        circle.insurance_balance -= circle.contribution_amount;
        circle.is_insurance_used = true;

        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);
    }

    fn propose_penalty_change(env: Env, user: Address, circle_id: u64, new_bps: u32) {
        user.require_auth();
        
        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();
        
        let member_key = DataKey::Member(user.clone());
        let member: Member = env.storage().instance().get(&member_key).expect("User is not a member");

        if !member.is_active {
            panic!("Member is ejected");
        }

        if new_bps > 10000 {
            panic!("Penalty cannot exceed 100%");
        }

        circle.proposed_late_fee_bps = new_bps;
        circle.proposal_votes_bitmap = 0;
        circle.proposal_votes_bitmap |= 1 << member.index;

        if circle.proposal_votes_bitmap.count_ones() > (circle.member_count as u32 / 2) {
            circle.late_fee_bps = circle.proposed_late_fee_bps;
            circle.proposed_late_fee_bps = 0;
            circle.proposal_votes_bitmap = 0;
        }

        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);
    }

    fn vote_penalty_change(env: Env, user: Address, circle_id: u64) {
        user.require_auth();

        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();
        
        let member_key = DataKey::Member(user.clone());
        let member: Member = env.storage().instance().get(&member_key).expect("User is not a member");

        if !member.is_active {
            panic!("Member is ejected");
        }

        if circle.proposed_late_fee_bps == 0 {
            panic!("No active proposal");
        }

        circle.proposal_votes_bitmap |= 1 << member.index;

        if circle.proposal_votes_bitmap.count_ones() > (circle.member_count as u32 / 2) {
            circle.late_fee_bps = circle.proposed_late_fee_bps;
            circle.proposed_late_fee_bps = 0;
            circle.proposal_votes_bitmap = 0;
        }

        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);
    }

    fn eject_member(env: Env, caller: Address, circle_id: u64, member: Address) {
        caller.require_auth();
        
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();
        
        if caller != circle.creator {
            panic!("Unauthorized: Only creator can eject members");
        }

        let member_key = DataKey::Member(member.clone());
        let mut member_info: Member = env.storage().instance().get(&member_key).expect("Member not found");

        if !member_info.is_active {
            panic!("Member already ejected");
        }

        member_info.is_active = false;
        env.storage().instance().set(&member_key, &member_info);

        // Remove member address by index mapping
        let member_by_index_key = DataKey::MemberByIndex(circle_id, member_info.index);
        env.storage().instance().remove(&member_by_index_key);

        // Deactivate vesting
        let vesting_key = DataKey::UserVesting(member.clone());
        if let Ok(mut vesting_info) = env.storage().instance().get::<DataKey, UserVestingInfo>(&vesting_key) {
            vesting_info.is_active = false;
            env.storage().instance().set(&vesting_key, &vesting_info);
        }

        let token_id = (circle_id as u128) << 64 | (member_info.index as u128);
        let client = SusuNftClient::new(&env, &circle.nft_contract);
        client.burn(&member, &token_id);
    }

    // --- CLAWBACK RECONCILIATION IMPLEMENTATIONS ---

    fn detect_and_handle_clawback(env: Env, caller: Address, circle_id: u64) {
        caller.require_auth();

        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();
        
        // Only circle creator or admin can detect clawbacks
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if caller != circle.creator && caller != admin {
            panic!("Unauthorized: Only creator or admin can detect clawbacks");
        }

        let token_client = token::Client::new(&env, &circle.token);
        let actual_balance = token_client.balance(&env.current_contract_address());
        
        if actual_balance < circle.expected_balance {
            let deficit_amount = circle.expected_balance - actual_balance;
            
            // Create deficit record
            let deficit = ClawbackDeficit {
                circle_id,
                deficit_amount,
                detection_timestamp: env.ledger().timestamp(),
                detected_by: caller.clone(),
                token_address: circle.token.clone(),
            };
            
            env.storage().instance().set(&DataKey::ClawbackDeficit(circle_id), &deficit);
            
            // Auto-pause round
            Self::pause_round(env.clone(), caller.clone(), circle_id, PauseReason::ClawbackDetected);
            
            // Emit clawback detection event
            env.events().publish(
                (Symbol::short("clawback_detected"), circle_id, caller),
                deficit_amount,
            );

            // Check for SBT holders and mark as dishonored if they were involved
            Self::handle_sbt_clawback_impact(env.clone(), circle_id);
        }
    }

    fn pause_round(env: Env, caller: Address, circle_id: u64, reason: PauseReason) {
        caller.require_auth();

        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();
        
        // Only circle creator or admin can pause rounds
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if caller != circle.creator && caller != admin {
            panic!("Unauthorized: Only creator or admin can pause rounds");
        }

        if circle.is_paused {
            panic!("Round is already paused");
        }

        circle.is_paused = true;
        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);

        let paused_round = PausedRound {
            circle_id,
            pause_timestamp: env.ledger().timestamp(),
            pause_reason: reason.clone(),
            paused_by: caller.clone(),
        };
        
        env.storage().instance().set(&DataKey::PausedRounds(circle_id), &paused_round);

        // Emit pause event
        env.events().publish(
            (Symbol::short("round_paused"), circle_id, caller),
            reason,
        );
    }

    fn propose_recovery_plan(env: Env, caller: Address, circle_id: u64, recovery_type: RecoveryType) {
        caller.require_auth();

        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();
        
        // Check if caller is a member or admin
        let member_key = DataKey::Member(caller.clone());
        let is_member = env.storage().instance().has(&member_key);
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        
        if !is_member && caller != admin {
            panic!("Unauthorized: Only members or admin can propose recovery plans");
        }

        let deficit_key = DataKey::ClawbackDeficit(circle_id);
        let deficit: ClawbackDeficit = env.storage().instance().get(&deficit_key)
            .unwrap_or_else(|| panic!("No deficit detected for this circle"));

        let recovery_plan = RecoveryPlan {
            circle_id,
            total_deficit: deficit.deficit_amount,
            recovery_type: recovery_type.clone(),
            proposed_by: caller.clone(),
            proposal_timestamp: env.ledger().timestamp(),
            votes_for: 0,
            votes_against: 0,
            is_active: true,
            recovery_contributions: Map::new(&env),
        };

        env.storage().instance().set(&DataKey::RecoveryPlan(circle_id), &recovery_plan);

        // Emit recovery plan proposal event
        env.events().publish(
            (Symbol::short("recovery_proposed"), circle_id, caller),
            recovery_type,
        );
    }

    fn vote_recovery_plan(env: Env, caller: Address, circle_id: u64, vote_for: bool) {
        caller.require_auth();

        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();
        let member_key = DataKey::Member(caller.clone());
        let member: Member = env.storage().instance().get(&member_key)
            .unwrap_or_else(|| panic!("User is not a member of this circle"));

        if !member.is_active {
            panic!("Member is ejected");
        }

        let mut recovery_plan: RecoveryPlan = env.storage().instance().get(&DataKey::RecoveryPlan(circle_id))
            .unwrap_or_else(|| panic!("No active recovery plan for this circle"));

        if !recovery_plan.is_active {
            panic!("Recovery plan is not active");
        }

        // Simple voting: each member gets one vote
        if vote_for {
            recovery_plan.votes_for += 1;
        } else {
            recovery_plan.votes_against += 1;
        }

        // Check if plan is approved (simple majority)
        let total_votes = recovery_plan.votes_for + recovery_plan.votes_against;
        if total_votes > (circle.member_count / 2) && recovery_plan.votes_for > recovery_plan.votes_against {
            // Plan approved - execute it
            Self::execute_recovery_plan(env.clone(), caller.clone(), circle_id);
        } else {
            env.storage().instance().set(&DataKey::RecoveryPlan(circle_id), &recovery_plan);
        }

        // Emit vote event
        env.events().publish(
            (Symbol::short("recovery_vote"), circle_id, caller),
            vote_for,
        );
    }

    fn contribute_to_recovery(env: Env, caller: Address, circle_id: u64, amount: u64) {
        caller.require_auth();

        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();
        let member_key = DataKey::Member(caller.clone());
        let member: Member = env.storage().instance().get(&member_key)
            .unwrap_or_else(|| panic!("User is not a member of this circle"));

        if !member.is_active {
            panic!("Member is ejected");
        }

        let mut recovery_plan: RecoveryPlan = env.storage().instance().get(&DataKey::RecoveryPlan(circle_id))
            .unwrap_or_else(|| panic!("No active recovery plan for this circle"));

        if !recovery_plan.is_active {
            panic!("Recovery plan is not active");
        }

        // Transfer recovery contribution
        let token_client = token::Client::new(&env, &circle.token);
        token_client.transfer(&caller, &env.current_contract_address(), &amount);

        // Record contribution
        let current_contribution = recovery_plan.recovery_contributions.get(caller.clone()).unwrap_or(0);
        recovery_plan.recovery_contributions.set(caller.clone(), current_contribution + amount);

        env.storage().instance().set(&DataKey::RecoveryPlan(circle_id), &recovery_plan);

        // Emit contribution event
        env.events().publish(
            (Symbol::short("recovery_contribution"), circle_id, caller),
            amount,
        );
    }

    fn execute_recovery_plan(env: Env, caller: Address, circle_id: u64) {
        caller.require_auth();

        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();
        let mut recovery_plan: RecoveryPlan = env.storage().instance().get(&DataKey::RecoveryPlan(circle_id))
            .unwrap_or_else(|| panic!("No active recovery plan for this circle"));

        if !recovery_plan.is_active {
            panic!("Recovery plan is not active");
        }

        match recovery_plan.recovery_type {
            RecoveryType::MemberContribution => {
                // Check if sufficient contributions have been made
                let mut total_contributions = 0u64;
                for (_, amount) in recovery_plan.recovery_contributions.iter() {
                    total_contributions += amount;
                }

                if total_contributions < recovery_plan.total_deficit {
                    panic!("Insufficient recovery contributions");
                }

                // Update expected balance to reflect new reality
                let token_client = token::Client::new(&env, &circle.token);
                circle.expected_balance = token_client.balance(&env.current_contract_address());
            },
            RecoveryType::InsuranceUsage => {
                // Use insurance funds to cover deficit
                if circle.insurance_balance < recovery_plan.total_deficit {
                    panic!("Insufficient insurance balance");
                }
                circle.insurance_balance -= recovery_plan.total_deficit;
            },
            RecoveryType::PayoutReduction => {
                // This would be handled in payout logic
                // For now, just mark the plan as executed
            },
            RecoveryType::Hybrid => {
                // Combination approach - implement as needed
                panic!("Hybrid recovery not yet implemented");
            },
        }

        // Deactivate recovery plan
        recovery_plan.is_active = false;
        env.storage().instance().set(&DataKey::RecoveryPlan(circle_id), &recovery_plan);
        
        // Clear deficit record
        env.storage().instance().remove(&DataKey::ClawbackDeficit(circle_id));

        // Emit recovery execution event
        env.events().publish(
            (Symbol::short("recovery_executed"), circle_id, caller),
            recovery_plan.total_deficit,
        );
    }

    fn resume_round(env: Env, caller: Address, circle_id: u64) {
        caller.require_auth();

        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();
        
        // Only circle creator or admin can resume rounds
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if caller != circle.creator && caller != admin {
            panic!("Unauthorized: Only creator or admin can resume rounds");
        }

        if !circle.is_paused {
            panic!("Round is not paused");
        }

        // Check if there's an active deficit
        let deficit_key = DataKey::ClawbackDeficit(circle_id);
        if env.storage().instance().has(&deficit_key) {
            panic!("Cannot resume: unresolved deficit exists");
        }

        circle.is_paused = false;
        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);

        // Remove pause record
        env.storage().instance().remove(&DataKey::PausedRounds(circle_id));

        // Emit resume event
        env.events().publish(
            (Symbol::short("round_resumed"), circle_id, caller),
            true,
        );
    }

    fn get_clawback_deficit(env: Env, circle_id: u64) -> ClawbackDeficit {
        env.storage().instance().get(&DataKey::ClawbackDeficit(circle_id))
            .unwrap_or_else(|| ClawbackDeficit {
                circle_id,
                deficit_amount: 0,
                detection_timestamp: 0,
                detected_by: Address::generate(&env),
                token_address: Address::generate(&env),
            })
    }

    fn get_recovery_plan(env: Env, circle_id: u64) -> RecoveryPlan {
        env.storage().instance().get(&DataKey::RecoveryPlan(circle_id))
            .unwrap_or_else(|| RecoveryPlan {
                circle_id,
                total_deficit: 0,
                recovery_type: RecoveryType::MemberContribution,
                proposed_by: Address::generate(&env),
                proposal_timestamp: 0,
                votes_for: 0,
                votes_against: 0,
                is_active: false,
                recovery_contributions: Map::new(&env),
            })
    }

    fn get_paused_round_info(env: Env, circle_id: u64) -> PausedRound {
        env.storage().instance().get(&DataKey::PausedRounds(circle_id))
            .unwrap_or_else(|| PausedRound {
                circle_id,
                pause_timestamp: 0,
                pause_reason: PauseReason::EmergencyMaintenance,
                paused_by: Address::generate(&env),
            })
    }

    // --- SOULBOUND TOKEN (SBT) IMPLEMENTATIONS (Issue #210) ---

    fn set_sbt_contract(env: Env, admin: Address, sbt_contract: Address) {
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .unwrap_or_else(|| panic!("Admin not set"));
        if admin != stored_admin {
            panic!("Unauthorized: Only admin can set SBT contract");
        }

        env.storage().instance().set(&DataKey::SbtContract, &sbt_contract);

        // Initialize default reputation milestones
        let milestone_5_cycles = ReputationMilestone {
            id: 1,
            name: Symbol::short("Reliable_Saver"),
            description: Symbol::short("Completed_5_cycles_reliably"),
            required_cycles: 5,
            min_reputation_score: 80,
            is_active: true,
        };
        env.storage().instance().set(&DataKey::ReputationMilestone(1), &milestone_5_cycles);

        let milestone_10_cycles = ReputationMilestone {
            id: 2,
            name: Symbol::short("Trusted_Member"),
            description: Symbol::short("Completed_10_cycles_reliably"),
            required_cycles: 10,
            min_reputation_score: 90,
            is_active: true,
        };
        env.storage().instance().set(&DataKey::ReputationMilestone(2), &milestone_10_cycles);

        env.events().publish(
            (Symbol::short("sbt_contract_set"), admin),
            sbt_contract,
        );
    }

    fn configure_reputation_milestone(env: Env, admin: Address, milestone_id: u32, milestone: ReputationMilestone) {
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .unwrap_or_else(|| panic!("Admin not set"));
        if admin != stored_admin {
            panic!("Unauthorized: Only admin can configure milestones");
        }

        env.storage().instance().set(&DataKey::ReputationMilestone(milestone_id), &milestone);

        env.events().publish(
            (Symbol::short("milestone_configured"), milestone_id),
            milestone.name,
        );
    }

    fn issue_sbt_credential(env: Env, user: Address, milestone_id: u32) {
        user.require_auth();

        let sbt_contract: Address = env.storage().instance().get(&DataKey::SbtContract)
            .unwrap_or_else(|| panic!("SBT contract not set"));

        let milestone: ReputationMilestone = env.storage().instance().get(&DataKey::ReputationMilestone(milestone_id))
            .unwrap_or_else(|| panic!("Milestone not found"));

        if !milestone.is_active {
            panic!("Milestone is not active");
        }

        // Check if user already has an SBT
        let user_sbt_key = DataKey::UserSbt(user.clone());
        if env.storage().instance().has(&user_sbt_key) {
            panic!("User already has an SBT credential");
        }

        // Check user's contribution history across all circles
        let user_reputation_score = Self::calculate_user_reputation_score(env.clone(), user.clone());
        
        if user_reputation_score < milestone.min_reputation_score {
            panic!("Insufficient reputation score");
        }

        // Check total cycles completed
        let total_cycles_completed = Self::get_user_total_cycles_completed(env.clone(), user.clone());
        if total_cycles_completed < milestone.required_cycles {
            panic!("Insufficient cycles completed");
        }

        // Generate unique token ID
        let token_id = (milestone_id as u128) << 96 | (env.ledger().timestamp() as u128);

        // Create SBT
        let sbt = SoulboundToken {
            token_id,
            owner: user.clone(),
            milestone_id,
            issued_at: env.ledger().timestamp(),
            status: SbtStatus::Active,
            reputation_score: user_reputation_score,
            cycles_completed: total_cycles_completed,
            metadata: milestone.name,
        };

        // Store SBT
        env.storage().instance().set(&user_sbt_key, &sbt);

        // Mint SBT on external contract
        let sbt_client = SbtTokenClient::new(&env, &sbt_contract);
        sbt_client.mint_sbt(&user, &token_id, &milestone.name);

        env.events().publish(
            (Symbol::short("sbt_issued"), user, milestone_id),
            token_id,
        );
    }

    fn revoke_sbt_credential(env: Env, admin: Address, user: Address, reason: Symbol) {
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .unwrap_or_else(|| panic!("Admin not set"));
        if admin != stored_admin {
            panic!("Unauthorized: Only admin can revoke SBT credentials");
        }

        let user_sbt_key = DataKey::UserSbt(user.clone());
        let mut sbt: SoulboundToken = env.storage().instance().get(&user_sbt_key)
            .unwrap_or_else(|| panic!("User has no SBT credential"));

        let sbt_contract: Address = env.storage().instance().get(&DataKey::SbtContract)
            .unwrap_or_else(|| panic!("SBT contract not set"));

        // Update status to revoked
        sbt.status = SbtStatus::Revoked;
        env.storage().instance().set(&user_sbt_key, &sbt);

        // Store revocation info
        let revocation_info = SbtRevocationInfo {
            user: user.clone(),
            token_id: sbt.token_id,
            revoked_at: env.ledger().timestamp(),
            reason: reason.clone(),
            revoked_by: admin.clone(),
        };
        env.storage().instance().set(&DataKey::SbtRevocationList(user.clone()), &revocation_info);

        // Burn SBT on external contract
        let sbt_client = SbtTokenClient::new(&env, &sbt_contract);
        sbt_client.burn(&sbt.token_id);

        env.events().publish(
            (Symbol::short("sbt_revoked"), user),
            reason,
        );
    }

    fn update_sbt_status(env: Env, admin: Address, user: Address, status: SbtStatus) {
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .unwrap_or_else(|| panic!("Admin not set"));
        if admin != stored_admin {
            panic!("Unauthorized: Only admin can update SBT status");
        }

        let user_sbt_key = DataKey::UserSbt(user.clone());
        let mut sbt: SoulboundToken = env.storage().instance().get(&user_sbt_key)
            .unwrap_or_else(|| panic!("User has no SBT credential"));

        let sbt_contract: Address = env.storage().instance().get(&DataKey::SbtContract)
            .unwrap_or_else(|| panic!("SBT contract not set"));

        sbt.status = status.clone();
        env.storage().instance().set(&user_sbt_key, &sbt);

        // Update metadata on external contract based on status
        let sbt_client = SbtTokenClient::new(&env, &sbt_contract);
        let metadata = match status {
            SbtStatus::Active => Symbol::short("Active"),
            SbtStatus::Dishonored => Symbol::short("Dishonored"),
            SbtStatus::Revoked => Symbol::short("Revoked"),
        };
        sbt_client.update_metadata(&sbt.token_id, &metadata);

        env.events().publish(
            (Symbol::short("sbt_status_updated"), user),
            status,
        );
    }

    fn get_user_sbt(env: Env, user: Address) -> SoulboundToken {
        let user_sbt_key = DataKey::UserSbt(user);
        env.storage().instance().get(&user_sbt_key)
            .unwrap_or_else(|| SoulboundToken {
                token_id: 0,
                owner: Address::generate(&env),
                milestone_id: 0,
                issued_at: 0,
                status: SbtStatus::Revoked,
                reputation_score: 0,
                cycles_completed: 0,
                metadata: Symbol::short("None"),
            })
    }

    fn get_reputation_milestone(env: Env, milestone_id: u32) -> ReputationMilestone {
        env.storage().instance().get(&DataKey::ReputationMilestone(milestone_id))
            .unwrap_or_else(|| ReputationMilestone {
                id: milestone_id,
                name: Symbol::short("Unknown"),
                description: Symbol::short("Milestone_not_found"),
                required_cycles: 0,
                min_reputation_score: 0,
                is_active: false,
            })
    }

    // --- BUSINESS GOAL VERIFICATION IMPLEMENTATIONS (Issue #212) ---

    fn set_business_goal(env: Env, creator: Address, circle_id: u64, goal_hash: Symbol, verified_vendor: Address, goal_amount: u64) {
        creator.require_auth();

        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id))
            .unwrap_or_else(|| panic!("Circle not found"));

        if creator != circle.creator {
            panic!("Unauthorized: Only circle creator can set business goals");
        }

        circle.business_goal_hash = Some(goal_hash.clone());
        circle.verified_vendor = Some(verified_vendor);
        circle.goal_amount = Some(goal_amount);
        circle.is_goal_verified = false;

        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);

        env.events().publish(
            (Symbol::short("business_goal_set"), circle_id, creator),
            goal_hash,
        );
    }

    fn verify_business_goal(env: Env, vendor: Address, circle_id: u64, invoice_hash: Symbol) {
        vendor.require_auth();

        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id))
            .unwrap_or_else(|| panic!("Circle not found"));

        let stored_vendor = circle.verified_vendor
            .unwrap_or_else(|| panic!("No verified vendor set for this circle"));

        if vendor != stored_vendor {
            panic!("Unauthorized: Only verified vendor can verify goals");
        }

        if circle.is_goal_verified {
            panic!("Goal already verified");
        }

        // In a real implementation, you would verify the invoice_hash matches the business_goal_hash
        // For now, we'll assume the verification is successful
        circle.is_goal_verified = true;

        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);

        env.events().publish(
            (Symbol::short("business_goal_verified"), circle_id, vendor),
            invoice_hash,
        );
    }

    fn release_goal_funds(env: Env, circle_id: u64) {
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id))
            .unwrap_or_else(|| panic!("Circle not found"));

        if !circle.is_goal_verified {
            panic!("Business goal not verified");
        }

        let goal_amount = circle.goal_amount
            .unwrap_or_else(|| panic!("No goal amount set"));

        // Find the current recipient (who should be the circle creator for business goals)
        let recipient = circle.creator;

        // Transfer funds to recipient
        let token_client = token::Client::new(&env, &circle.token);
        token_client.transfer(&env.current_contract_address(), &recipient, &goal_amount);

        // Reset goal verification
        let mut updated_circle = circle;
        updated_circle.is_goal_verified = false;
        updated_circle.business_goal_hash = None;
        updated_circle.goal_amount = None;
        env.storage().instance().set(&DataKey::Circle(circle_id), &updated_circle);

        env.events().publish(
            (Symbol::short("goal_funds_released"), circle_id, recipient),
            goal_amount,
        );
    }

    fn get_business_goal_info(env: Env, circle_id: u64) -> (Option<Symbol>, Option<Address>, Option<u64>, bool) {
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id))
            .unwrap_or_else(|| CircleInfo {
                id: circle_id,
                creator: Address::generate(&env),
                contribution_amount: 0,
                max_members: 0,
                member_count: 0,
                current_recipient_index: 0,
                is_active: false,
                token: Address::generate(&env),
                deadline_timestamp: 0,
                cycle_duration: 0,
                contribution_bitmap: 0,
                payout_bitmap: 0,
                insurance_balance: 0,
                insurance_fee_bps: 0,
                is_insurance_used: false,
                late_fee_bps: 0,
                proposed_late_fee_bps: 0,
                proposal_votes_bitmap: 0,
                nft_contract: Address::generate(&env),
                cycle_count: 0,
                is_paused: false,
                expected_balance: 0,
                organizer_fee_bps: 0,
                business_goal_hash: None,
                verified_vendor: None,
                goal_amount: None,
                is_goal_verified: false,
            });

        (circle.business_goal_hash, circle.verified_vendor, circle.goal_amount, circle.is_goal_verified)
    }

    // --- STELLAR ANCHOR INTERFACE IMPLEMENTATION ---

    fn register_anchor(env: Env, admin: Address, anchor_address: Address, name: Symbol, sep_version: Symbol, kyc_required: bool, supported_tokens: Vec<Address>, max_deposit_amount: u64, daily_deposit_limit: u64) {
        admin.require_auth();

        // Verify admin authorization
        let contract_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != contract_admin {
            panic!("Unauthorized: Only admin can register anchors");
        }

        // Create anchor info
        let anchor_info = AnchorInfo {
            address: anchor_address.clone(),
            name,
            sep_version,
            status: AnchorStatus::Active,
            registration_timestamp: env.ledger().timestamp(),
            kyc_required,
            supported_tokens,
            max_deposit_amount,
            daily_deposit_limit,
        };

        // Store in registry
        let mut registry: Vec<Address> = env.storage().instance().get(&DataKey::AnchorRegistry).unwrap_or(Vec::new(&env));
        if !registry.contains(&anchor_address) {
            registry.push_back(anchor_address.clone());
            env.storage().instance().set(&DataKey::AnchorRegistry, &registry);
        }

        // Store anchor info
        let anchor_info_key = DataKey::AnchorInfo(anchor_address);
        env.storage().instance().set(&anchor_info_key, &anchor_info);

        // Emit registration event
        env.events().publish(
            (Symbol::short("anchor_registered"), anchor_address, admin),
            sep_version,
        );
    }

    fn deposit_for_user(env: Env, anchor: Address, beneficiary: Address, circle_id: u64, amount: u64, token_address: Address, fiat_reference: Symbol) {
        anchor.require_auth();

        // Verify anchor is registered and active
        let anchor_info_key = DataKey::AnchorInfo(anchor.clone());
        let anchor_info: AnchorInfo = env.storage().instance().get(&anchor_info_key)
            .unwrap_or_else(|| panic!("Anchor not registered"));

        if anchor_info.status != AnchorStatus::Active {
            panic!("Anchor is not active");
        }

        // Verify token is supported
        if !anchor_info.supported_tokens.contains(&token_address) {
            panic!("Token not supported by anchor");
        }

        // Check deposit limits
        if amount > anchor_info.max_deposit_amount {
            panic!("Deposit amount exceeds maximum limit");
        }

        // Get circle info
        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id))
            .unwrap_or_else(|| panic!("Circle not found"));

        if circle.is_paused {
            panic!("Circle is paused due to clawback detection");
        }

        // Verify beneficiary is a member of the circle
        let member_key = DataKey::Member(beneficiary.clone());
        let member: Member = env.storage().instance().get(&member_key)
            .unwrap_or_else(|| panic!("Beneficiary is not a member of this circle"));

        if !member.is_active {
            panic!("Beneficiary member is not active");
        }

        // Generate unique deposit ID
        let deposit_id = env.ledger().timestamp() + (circle_id << 32);

        // Create deposit record
        let deposit = AnchorDeposit {
            deposit_id,
            anchor_address: anchor.clone(),
            beneficiary: beneficiary.clone(),
            circle_id,
            amount,
            token_address: token_address.clone(),
            fiat_reference,
            status: DepositStatus::Pending,
            timestamp: env.ledger().timestamp(),
            gas_used: 0,
            error_message: None,
        };

        // Store deposit
        env.storage().instance().set(&DataKey::PendingDeposit(deposit_id), &deposit);

        // Process the deposit (similar to regular deposit but with anchor as relayer)
        let token_client = token::Client::new(&env, &token_address);

        // Calculate insurance fee
        let insurance_fee = ((amount as u128 * circle.insurance_fee_bps as u128) / 10000) as u64;
        let total_amount = amount + insurance_fee;

        // Update expected balance before transfer
        circle.expected_balance += total_amount;

        // Transfer from anchor to contract
        token_client.transfer(&anchor, &env.current_contract_address(), &total_amount);

        if insurance_fee > 0 {
            circle.insurance_balance += insurance_fee;
        }

        // Update member contribution
        member.contribution_count += 1;
        member.last_contribution_time = env.ledger().timestamp();
        env.storage().instance().set(&member_key, &member);

        // Update circle
        circle.deadline_timestamp = env.ledger().timestamp() + circle.cycle_duration;
        circle.contribution_bitmap |= 1 << member.index;
        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);

        // Update deposit status to completed
        let mut completed_deposit = deposit;
        completed_deposit.status = DepositStatus::Completed;
        env.storage().instance().set(&DataKey::PendingDeposit(deposit_id), &completed_deposit);

        // Mine governance tokens if enabled
        Self::mine_governance_tokens(env.clone(), beneficiary.clone(), circle_id, &mut circle, &mut Member { ..member });

        // Check cycle completion
        Self::check_and_complete_cycle(env.clone(), circle_id);

        // Check for SBT credential
        Self::check_and_issue_sbt_credential(env.clone(), beneficiary.clone());

        // Emit events
        env.events().publish(
            (Symbol::short("anchor_deposit_completed"), anchor, beneficiary),
            (deposit_id, amount),
        );
    }

    fn configure_anchor_deposit(env: Env, anchor: Address, circle_id: u64, auto_deposit_enabled: bool, gas_subsidy_amount: u64, fee_bps: u32) {
        anchor.require_auth();

        // Verify anchor is registered
        let anchor_info_key = DataKey::AnchorInfo(anchor.clone());
        let _anchor_info: AnchorInfo = env.storage().instance().get(&anchor_info_key)
            .unwrap_or_else(|| panic!("Anchor not registered"));

        // Create or update deposit config
        let config = AnchorDepositConfig {
            anchor_address: anchor.clone(),
            circle_id,
            auto_deposit_enabled,
            gas_subsidy_amount,
            fee_bps,
            last_deposit_timestamp: 0,
            total_deposits_made: 0,
        };

        // Store config
        let anchor_config_key = DataKey::AnchorConfig(anchor.clone());
        env.storage().instance().set(&anchor_config_key, &config);

        // Emit configuration event
        env.events().publish(
            (Symbol::short("anchor_config_updated"), anchor, circle_id),
            auto_deposit_enabled,
        );
    }

    fn get_anchor_info(env: Env, anchor_address: Address) -> AnchorInfo {
        let anchor_info_key = DataKey::AnchorInfo(anchor_address);
        env.storage().instance().get(&anchor_info_key)
            .unwrap_or_else(|| panic!("Anchor not found"))
    }

    fn get_deposit_status(env: Env, deposit_id: u64) -> AnchorDeposit {
        env.storage().instance().get(&DataKey::PendingDeposit(deposit_id))
            .unwrap_or_else(|| panic!("Deposit not found"))
    }

    fn get_registered_anchors(env: Env) -> Vec<Address> {
        env.storage().instance().get(&DataKey::AnchorRegistry).unwrap_or(Vec::new(&env))
    }

    // --- SBT CREDENTIAL IMPLEMENTATION ---

    fn initialize_sbt_system(env: Env, admin: Address, sbt_contract: Address) {
        admin.require_auth();

        // Verify admin authorization
        let contract_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != contract_admin {
            panic!("Unauthorized: Only admin can initialize SBT system");
        }

        // Set SBT contract address
        env.storage().instance().set(&DataKey::SbtContract, &sbt_contract);

        // Create default reputation milestone (5 cycles, 80 reputation)
        let milestone = ReputationMilestone {
            id: 1,
            name: Symbol::short("Reliable_Saver"),
            description: Symbol::short("Completed_5_cycles_with_80+ reputation"),
            required_cycles: 5,
            min_reputation_score: 80,
            is_active: true,
        };

        env.storage().instance().set(&DataKey::ReputationMilestone(1), &milestone);

        // Emit initialization event
        env.events().publish(
            (Symbol::short("sbt_system_initialized"), admin, sbt_contract),
            true,
        );
    }

    fn create_reputation_milestone(env: Env, admin: Address, milestone_id: u32, name: Symbol, description: Symbol, required_cycles: u32, min_reputation_score: u32) {
        admin.require_auth();

        // Verify admin authorization
        let contract_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != contract_admin {
            panic!("Unauthorized: Only admin can create milestones");
        }

        // Create milestone
        let milestone = ReputationMilestone {
            id: milestone_id,
            name,
            description,
            required_cycles,
            min_reputation_score,
            is_active: true,
        };

        // Store milestone
        env.storage().instance().set(&DataKey::ReputationMilestone(milestone_id), &milestone);

        // Emit milestone creation event
        env.events().publish(
            (Symbol::short("milestone_created"), admin, milestone_id),
            required_cycles,
        );
    }

    fn issue_sbt_credential(env: Env, admin: Address, user: Address, milestone_id: u32) {
        admin.require_auth();

        // Verify admin authorization
        let contract_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != contract_admin {
            panic!("Unauthorized: Only admin can issue SBT credentials");
        }

        // Check if SBT system is initialized
        let sbt_contract: Address = env.storage().instance().get(&DataKey::SbtContract)
            .unwrap_or_else(|| panic!("SBT system not initialized"));

        // Check if user already has SBT
        let user_sbt_key = DataKey::UserSbt(user.clone());
        if env.storage().instance().has(&user_sbt_key) {
            panic!("User already has an SBT credential");
        }

        // Get milestone
        let milestone: ReputationMilestone = env.storage().instance().get(&DataKey::ReputationMilestone(milestone_id))
            .unwrap_or_else(|| panic!("Milestone not found"));

        if !milestone.is_active {
            panic!("Milestone is not active");
        }

        // Calculate user's reputation and cycles
        let reputation_score = Self::calculate_user_reputation_score(env.clone(), user.clone());
        let total_cycles = Self::get_user_total_cycles_completed(env.clone(), user.clone());

        // Verify user meets requirements
        if total_cycles < milestone.required_cycles || reputation_score < milestone.min_reputation_score {
            panic!("User does not meet milestone requirements");
        }

        // Create SBT
        let token_id = (milestone_id as u128) << 96 | (env.ledger().timestamp() as u128);
        
        let sbt = SoulboundToken {
            token_id,
            owner: user.clone(),
            milestone_id,
            issued_at: env.ledger().timestamp(),
            status: SbtStatus::Active,
            reputation_score,
            cycles_completed: total_cycles,
            metadata: milestone.name,
        };

        // Store SBT
        env.storage().instance().set(&user_sbt_key, &sbt);

        // Mint SBT on external contract
        let sbt_client = SbtTokenClient::new(&env, &sbt_contract);
        sbt_client.mint_sbt(&user, &token_id, &milestone.name);

        // Emit event
        env.events().publish(
            (Symbol::short("sbt_issued"), user, milestone_id),
            token_id,
        );
    }

    fn revoke_sbt_credential(env: Env, admin: Address, user: Address, reason: Symbol) {
        admin.require_auth();

        // Verify admin authorization
        let contract_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != contract_admin {
            panic!("Unauthorized: Only admin can revoke SBT credentials");
        }

        // Get user's SBT
        let user_sbt_key = DataKey::UserSbt(user.clone());
        let mut sbt: SoulboundToken = env.storage().instance().get(&user_sbt_key)
            .unwrap_or_else(|| panic!("User does not have an SBT credential"));

        if sbt.status == SbtStatus::Revoked {
            panic!("SBT is already revoked");
        }

        // Update status
        sbt.status = SbtStatus::Revoked;
        env.storage().instance().set(&user_sbt_key, &sbt);

        // Get SBT contract
        let sbt_contract: Address = env.storage().instance().get(&DataKey::SbtContract)
            .unwrap_or_else(|| panic!("SBT system not initialized"));

        // Update metadata on external contract
        let sbt_client = SbtTokenClient::new(&env, &sbt_contract);
        sbt_client.update_metadata(&sbt.token_id, &Symbol::short("Revoked"));

        // Store revocation info
        let revocation_info = SbtRevocationInfo {
            user: user.clone(),
            token_id: sbt.token_id,
            revoked_at: env.ledger().timestamp(),
            reason,
            revoked_by: admin.clone(),
        };

        env.storage().instance().set(&DataKey::SbtRevocationList(user.clone()), &revocation_info);

        // Emit event
        env.events().publish(
            (Symbol::short("sbt_revoked"), user, admin),
            (sbt.token_id, reason),
        );
    }

    fn update_sbt_status(env: Env, admin: Address, user: Address, status: SbtStatus) {
        admin.require_auth();

        // Verify admin authorization
        let contract_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != contract_admin {
            panic!("Unauthorized: Only admin can update SBT status");
        }

        // Get user's SBT
        let user_sbt_key = DataKey::UserSbt(user.clone());
        let mut sbt: SoulboundToken = env.storage().instance().get(&user_sbt_key)
            .unwrap_or_else(|| panic!("User does not have an SBT credential"));

        // Update status
        sbt.status = status;
        env.storage().instance().set(&user_sbt_key, &sbt);

        // Get SBT contract
        let sbt_contract: Address = env.storage().instance().get(&DataKey::SbtContract)
            .unwrap_or_else(|| panic!("SBT system not initialized"));

        // Update metadata on external contract
        let status_symbol = match status {
            SbtStatus::Active => Symbol::short("Active"),
            SbtStatus::Dishonored => Symbol::short("Dishonored"),
            SbtStatus::Revoked => Symbol::short("Revoked"),
        };

        let sbt_client = SbtTokenClient::new(&env, &sbt_contract);
        sbt_client.update_metadata(&sbt.token_id, &status_symbol);

        // Emit event
        env.events().publish(
            (Symbol::short("sbt_status_updated"), user, admin),
            (sbt.token_id, status_symbol),
        );
    }

    fn get_user_sbt(env: Env, user: Address) -> Option<SoulboundToken> {
        let user_sbt_key = DataKey::UserSbt(user);
        env.storage().instance().get(&user_sbt_key)
    }

    fn get_reputation_milestone(env: Env, milestone_id: u32) -> ReputationMilestone {
        env.storage().instance().get(&DataKey::ReputationMilestone(milestone_id))
            .unwrap_or_else(|| ReputationMilestone {
                id: milestone_id,
                name: Symbol::short("Unknown"),
                description: Symbol::short("Milestone not found"),
                required_cycles: 0,
                min_reputation_score: 0,
                is_active: false,
            })
    }

    fn verify_user_reputation(env: Env, user: Address) -> (u32, bool) {
        let reputation_score = Self::calculate_user_reputation_score(env.clone(), user.clone());
        let has_sbt = env.storage().instance().has(&DataKey::UserSbt(user.clone()));
        (reputation_score, has_sbt)
    }
}

// --- PRIVATE HELPER FUNCTIONS ---

impl SoroSusu {
    fn get_current_recipient(env: Env, circle_id: u64) -> Address {
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();
        
        let current_recipient_index = circle.current_recipient_index;
        let member_by_index_key = DataKey::MemberByIndex(circle_id, current_recipient_index as u32);
        
        env.storage().instance().get(&member_by_index_key)
            .unwrap_or_else(|| panic!("Member not found at index {}", current_recipient_index))
    }

    fn mine_governance_tokens(env: Env, user: Address, circle_id: u64, circle: &mut CircleInfo, member: &mut Member) {
        let config: MiningConfig = env.storage().instance().get(&DataKey::MiningConfig).unwrap();
        
        if !config.is_mining_enabled {
            return;
        }

        let governance_token: Address = env.storage().instance().get(&DataKey::GovernanceToken);
        if governance_token.is_none() {
            return;
        }

        let governance_token = governance_token.unwrap();

        // Check if user has already mined for this contribution cycle
        let contribution_key = DataKey::Deposit(circle_id, user.clone());
        if env.storage().instance().has(&contribution_key) {
            return; // Already mined for this contribution
        }

        // Calculate mining amount
        let mining_amount = config.tokens_per_contribution;
        
        // Check max mining per circle
        let total_mined: u64 = env.storage().instance().get(&DataKey::TotalMinedTokens).unwrap_or(0);
        if total_mined + mining_amount > config.max_mining_per_circle {
            return; // Max mining reached for this circle
        }

        // Update user vesting
        let vesting_key = DataKey::UserVesting(user.clone());
        let mut vesting_info: UserVestingInfo = env.storage().instance().get(&vesting_key).unwrap();
        
        if !vesting_info.is_active {
            vesting_info.start_cycle = circle.cycle_count;
            vesting_info.is_active = true;
        }
        
        vesting_info.total_allocated += mining_amount;
        vesting_info.contributions_made += 1;
        env.storage().instance().set(&vesting_key, &vesting_info);

        // Update user stats
        let stats_key = DataKey::UserMiningStats(user.clone());
        let mut stats: UserMiningStats = env.storage().instance().get(&stats_key).unwrap();
        stats.total_contributions += 1;
        stats.total_tokens_earned += mining_amount;
        stats.last_mining_timestamp = env.ledger().timestamp();
        env.storage().instance().set(&stats_key, &stats);

        // Update total mined tokens
        env.storage().instance().set(&DataKey::TotalMinedTokens, &(total_mined + mining_amount));

        // Mark as mined for this contribution
        env.storage().instance().set(&contribution_key, &true);

        // Mint tokens to vesting vault (contract holds them)
        let token_client = token::Client::new(&env, &governance_token);
        token_client.mint(&env.current_contract_address(), &mining_amount);

        // Emit mining event
        env.events().publish(
            (Symbol::short("tokens_mined"), user.clone(), circle_id),
            mining_amount,
        );
    }

    fn check_and_complete_cycle(env: Env, circle_id: u64) {
        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();
        
        // Check if all active members have contributed
        let required_contributions = circle.member_count;
        let current_contributions = circle.contribution_bitmap.count_ones() as u16;
        
        if current_contributions >= required_contributions {
            // Cycle complete - increment cycle count
            circle.cycle_count += 1;
            
            // Reset contribution bitmap for next cycle
            circle.contribution_bitmap = 0;
            circle.is_insurance_used = false;
            
            // Update deadline for next cycle
            circle.deadline_timestamp = env.ledger().timestamp() + circle.cycle_duration;
            
            env.storage().instance().set(&DataKey::Circle(circle_id), &circle);
            
            // Emit cycle completion event
            env.events().publish(
                (Symbol::short("cycle_completed"), circle_id),
                circle.cycle_count,
            );
        }
    }

    fn calculate_vested_amount(
        total_allocated: u64,
        start_cycle: u32,
        current_cycle: u32,
        contributions_made: u32,
    ) -> u64 {
        if current_cycle <= start_cycle {
            return 0;
        }

        let cycles_passed = current_cycle - start_cycle;
        let config: MiningConfig = Env::default().storage().instance().get(&DataKey::MiningConfig).unwrap();
        
        if cycles_passed <= config.cliff_cycles {
            return 0;
        }

        let vesting_cycles = config.vesting_duration_cycles;
        if cycles_passed >= vesting_cycles {
            return total_allocated;
        }

        let vesting_progress = cycles_passed - config.cliff_cycles;
        let total_vesting_cycles = vesting_cycles - config.cliff_cycles;
        
        let vested_amount = (total_allocated as u128 * vesting_progress as u128) / total_vesting_cycles as u128;
        vested_amount as u64
    }

    fn get_current_global_cycle(env: Env) -> u32 {
        // Simple implementation: use ledger timestamp / average cycle duration
        let avg_cycle_duration = 604800; // 1 week in seconds
        let current_timestamp = env.ledger().timestamp();
        (current_timestamp / avg_cycle_duration) as u32
    }

    // --- SBT HELPER FUNCTIONS ---

    fn calculate_user_reputation_score(env: Env, user: Address) -> u32 {
        let member_key = DataKey::Member(user.clone());
        let member: Member = env.storage().instance().get(&member_key)
            .unwrap_or_else(|| Member {
                address: user,
                index: 0,
                contribution_count: 0,
                last_contribution_time: 0,
                is_active: false,
            });

        if !member.is_active {
            return 0;
        }

        // Base score from contribution count (max 50 points)
        let contribution_score = (member.contribution_count.min(50) as u32) * 1;

        // Bonus for timely payments (max 30 points)
        let timely_bonus = Self::calculate_timely_payment_bonus(env.clone(), user.clone());

        // Bonus for active participation (max 20 points)
        let participation_bonus = Self::calculate_participation_bonus(env.clone(), user.clone());

        let total_score = contribution_score + timely_bonus + participation_bonus;
        total_score.min(100) // Cap at 100
    }

    fn calculate_timely_payment_bonus(env: Env, user: Address) -> u32 {
        // Check if user has any late fees in their history
        // This is a simplified implementation - in practice you'd track payment history
        let member_key = DataKey::Member(user);
        if let Ok(member) = env.storage().instance().get::<DataKey, Member>(&member_key) {
            if member.contribution_count > 0 {
                // Assume most payments are timely for this example
                return 25;
            }
        }
        0
    }

    fn calculate_participation_bonus(env: Env, user: Address) -> u32 {
        // Bonus for being active in multiple circles or long-term participation
        let member_key = DataKey::Member(user);
        if let Ok(member) = env.storage().instance().get::<DataKey, Member>(&member_key) {
            if member.contribution_count >= 10 {
                return 20;
            } else if member.contribution_count >= 5 {
                return 10;
            } else if member.contribution_count >= 2 {
                return 5;
            }
        }
        0
    }

    fn get_user_total_cycles_completed(env: Env, user: Address) -> u32 {
        let member_key = DataKey::Member(user);
        if let Ok(member) = env.storage().instance().get::<DataKey, Member>(&member_key) {
            return member.contribution_count;
        }
        0
    }

    fn check_and_issue_sbt_credential(env: Env, user: Address) {
        // Check if SBT contract is set
        let sbt_contract: Option<Address> = env.storage().instance().get(&DataKey::SbtContract);
        if sbt_contract.is_none() {
            return; // SBT system not initialized
        }

        // Check if user already has an SBT
        let user_sbt_key = DataKey::UserSbt(user.clone());
        if env.storage().instance().has(&user_sbt_key) {
            return; // User already has SBT
        }

        // Calculate user's reputation score and cycles
        let reputation_score = Self::calculate_user_reputation_score(env.clone(), user.clone());
        let total_cycles = Self::get_user_total_cycles_completed(env.clone(), user.clone());

        // Check for milestone 1 (5 cycles, 80 reputation)
        if total_cycles >= 5 && reputation_score >= 80 {
            let milestone_id = 1u32;
            if let Ok(milestone) = env.storage().instance().get::<DataKey, ReputationMilestone>(&DataKey::ReputationMilestone(milestone_id)) {
                if milestone.is_active {
                    // Auto-issue SBT credential
                    let token_id = (milestone_id as u128) << 96 | (env.ledger().timestamp() as u128);
                    
                    let sbt = SoulboundToken {
                        token_id,
                        owner: user.clone(),
                        milestone_id,
                        issued_at: env.ledger().timestamp(),
                        status: SbtStatus::Active,
                        reputation_score,
                        cycles_completed: total_cycles,
                        metadata: milestone.name,
                    };

                    // Store SBT
                    env.storage().instance().set(&user_sbt_key, &sbt);

                    // Mint SBT on external contract
                    let sbt_client = SbtTokenClient::new(&env, &sbt_contract.unwrap());
                    sbt_client.mint_sbt(&user, &token_id, &milestone.name);

                    // Emit event
                    env.events().publish(
                        (Symbol::short("sbt_auto_issued"), user, milestone_id),
                        token_id,
                    );
                }
            }
        }
    }

    fn handle_sbt_clawback_impact(env: Env, circle_id: u64) {
        // Get all members of the circle and check if any have SBTs
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id))
            .unwrap_or_else(|| panic!("Circle not found"));

        let sbt_contract: Option<Address> = env.storage().instance().get(&DataKey::SbtContract);
        if sbt_contract.is_none() {
            return; // SBT system not initialized
        }

        // Check each member for SBT and potentially mark as dishonored
        for i in 0..circle.member_count {
            let member_by_index_key = DataKey::MemberByIndex(circle_id, i as u32);
            if let Ok(member_address) = env.storage().instance().get::<DataKey, Address>(&member_by_index_key) {
                let user_sbt_key = DataKey::UserSbt(member_address.clone());
                if let Ok(mut sbt) = env.storage().instance().get::<DataKey, SoulboundToken>(&user_sbt_key) {
                    // Mark SBT as dishonored due to clawback involvement
                    if sbt.status == SbtStatus::Active {
                        sbt.status = SbtStatus::Dishonored;
                        env.storage().instance().set(&user_sbt_key, &sbt);

                        // Update metadata on external contract
                        let sbt_client = SbtTokenClient::new(&env, &sbt_contract.unwrap());
                        sbt_client.update_metadata(&sbt.token_id, &Symbol::short("Dishonored"));

                        // Emit event
                        env.events().publish(
                            (Symbol::short("sbt_dishonored"), member_address, circle_id),
                            sbt.token_id,
                        );
                    }
                }
            }
        }
    }
}

// --- TESTS ---
#[cfg(test)]
mod clawback_tests;

#[cfg(test)]
mod commission_tests;

#[cfg(test)]
mod anchor_tests;
