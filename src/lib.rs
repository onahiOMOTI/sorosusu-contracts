#![no_std]
use soroban_sdk::{contract, contracttype, contractimpl, Address, Env, Vec, Symbol, token, String};
use soroban_sdk::testutils::{Address as TestAddress, Arbitrary as TestArbitrary};
use soroban_sdk::arbitrary::{Arbitrary, Unstructured};

pub mod receipt;
pub mod goal_escrow;           // ← NEW: Goal Escrow Module

// --- DATA STRUCTURES ---
const TAX_WITHHOLDING_BPS: u64 = 1000; // 10%

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Circle(u64),
    Member(u64, Address),
    CircleCount,
    Deposit(u64, Address),
    GroupReserve,
    // #225: Duration Proposals
    Proposal(u64, u64),
    ProposalCount(u64),
    Vote(u64, u64, Address),
    // #227: Bond Storage
    Bond(u64),
    // #228: Governance
    Stake(Address),
    GlobalFeeBP,
    // #234: Goal Escrow Storage
    GoalEscrow(u32),           // Escrow ID → GoalEscrow
    NextEscrowId,              // Counter for escrow IDs
}

#[contracttype]
#[derive(Clone, Debug)]
pub enum EscrowStatus {
    PendingInvoice,
    AwaitingDelivery,
    Delivered,
    Cancelled,
}

#[contracttype]
#[derive(Clone)]
pub struct GoalEscrow {
    pub id: u32,
    pub winner: Address,
    pub group_id: u32,
    pub amount: u128,
    pub asset: Address,
    pub vendor: Address,
    pub invoice_reference: String,
    pub status: EscrowStatus,
    pub created_at: u64,
    pub delivery_confirmed_at: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct DurationProposal {
    pub id: u64,
    pub new_duration: u64,
    pub votes_for: u16,
    pub votes_against: u16,
    pub end_time: u64,
    pub is_active: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct Member {
    pub address: Address,
    pub has_contributed: bool,
    pub contribution_count: u32,
    pub last_contribution_time: u64,
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
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TaxReport {
    pub circle_id: u64,
    pub user: Address,
    pub gross_interest_total_for_circle: u64,
    pub gross_interest_for_user: u64,
    pub total_tax_withheld_for_circle: u64,
    pub total_tax_withheld_for_user: u64,
    pub total_tax_claimed_for_circle: u64,
    pub total_tax_claimed_for_user: u64,
    pub current_tax_vault_balance: u64,
}

// --- CONTRACT TRAIT ---

pub trait SoroSusuTrait {
    fn init(env: Env, admin: Address, global_fee: u32);
    
    fn create_circle(env: Env, creator: Address, amount: u64, max_members: u16, token: Address, cycle_duration: u64, bond_amount: u64) -> u64;

    fn join_circle(env: Env, user: Address, circle_id: u64);

    fn deposit(env: Env, user: Address, circle_id: u64, rounds: u32);

    // #225: Variable Round Duration
    fn propose_duration(env: Env, user: Address, circle_id: u64, new_duration: u64) -> u64;
    fn vote_duration(env: Env, user: Address, circle_id: u64, proposal_id: u64, approve: bool);

    // #227: Bond Management
    fn slash_bond(env: Env, admin: Address, circle_id: u64);
    fn release_bond(env: Env, admin: Address, circle_id: u64);

    // #228: XLM Staking & Governance
    fn stake_xlm(env: Env, user: Address, xlm_token: Address, amount: u64);
    fn unstake_xlm(env: Env, user: Address, xlm_token: Address, amount: u64);
    fn update_global_fee(env: Env, admin: Address, new_fee: u32);

    // #233: Receipt Generator
    fn generate_receipt(
        env: Env,
        contributor: Address,
        group_id: u32,
        amount: u128,
        asset_code: String,
        group_name: String,
    ) -> String;

    // #234: Sub-Susu Goal Escrow (Vendor-Direct Payout)
    fn create_goal_escrow(
        env: Env,
        group_id: u32,
        winner: Address,
        amount: u128,
        asset: Address,
        vendor: Address,
        invoice_reference: String,
    ) -> u32;

    fn confirm_delivery(env: Env, escrow_id: u32);

    fn get_goal_escrow(env: Env, escrow_id: u32) -> GoalEscrow;

    fn cancel_goal_escrow(env: Env, escrow_id: u32);
}

// --- IMPLEMENTATION ---

#[contract]
pub struct SoroSusu;

#[contractimpl]
impl SoroSusuTrait for SoroSusu {
    // ... (all your existing functions remain unchanged: init, create_circle, join_circle, deposit, propose_duration, vote_duration, slash_bond, release_bond, stake_xlm, unstake_xlm, update_global_fee, generate_receipt)

    // Keep all your existing implementations here...
    // (I'm omitting them for brevity — they stay exactly as you had them)

    // ==================== NEW: GOAL ESCROW FUNCTIONS (#234) ====================

    fn create_goal_escrow(
        env: Env,
        group_id: u32,
        winner: Address,
        amount: u128,
        asset: Address,
        vendor: Address,
        invoice_reference: String,
    ) -> u32 {
        winner.require_auth(); // Winner must authorize

        let mut next_id: u32 = env.storage().instance().get(&DataKey::NextEscrowId).unwrap_or(0);
        next_id += 1;

        let escrow = GoalEscrow {
            id: next_id,
            winner: winner.clone(),
            group_id,
            amount,
            asset: asset.clone(),
            vendor: vendor.clone(),
            invoice_reference,
            status: EscrowStatus::AwaitingDelivery,
            created_at: env.ledger().timestamp(),
            delivery_confirmed_at: None,
        };

        // Transfer funds into escrow (from contract balance)
        let token_client = token::Client::new(&env, &asset);
        token_client.transfer(&env.current_contract_address(), &env.current_contract_address(), &(amount as i128)); // Self-transfer to lock

        env.storage().instance().set(&DataKey::GoalEscrow(next_id), &escrow);
        env.storage().instance().set(&DataKey::NextEscrowId, &next_id);

        env.events().publish((Symbol::new(&env, "goal_escrow_created"),), (next_id, winner, amount, vendor));

        next_id
    }

    fn confirm_delivery(env: Env, escrow_id: u32) {
        let mut escrow: GoalEscrow = env.storage().instance().get(&DataKey::GoalEscrow(escrow_id))
            .unwrap_or_else(|| panic!("Escrow not found"));

        escrow.winner.require_auth();

        if escrow.status != EscrowStatus::AwaitingDelivery {
            panic!("Invalid escrow state");
        }

        // Release funds to vendor
        let token_client = token::Client::new(&env, &escrow.asset);
        token_client.transfer(&env.current_contract_address(), &escrow.vendor, &(escrow.amount as i128));

        escrow.status = EscrowStatus::Delivered;
        escrow.delivery_confirmed_at = Some(env.ledger().timestamp());

        env.storage().instance().set(&DataKey::GoalEscrow(escrow_id), &escrow);

        env.events().publish((Symbol::new(&env, "goal_escrow_delivered"),), (escrow_id, escrow.vendor, escrow.amount));
    }

    fn get_goal_escrow(env: Env, escrow_id: u32) -> GoalEscrow {
        env.storage().instance().get(&DataKey::GoalEscrow(escrow_id))
            .unwrap_or_else(|| panic!("Escrow not found"))
    }

    fn cancel_goal_escrow(env: Env, escrow_id: u32) {
        // TODO: Implement admin or timeout-based cancellation
        // For now, stub
        panic!("Cancel not yet implemented");
    }

    // Keep your existing generate_receipt function
    fn generate_receipt(
        env: Env,
        contributor: Address,
        group_id: u32,
        amount: u128,
        asset_code: String,
        group_name: String,
    ) -> String {
        Self::generate_receipt(env, contributor, group_id, amount, asset_code, group_name)
    }
}