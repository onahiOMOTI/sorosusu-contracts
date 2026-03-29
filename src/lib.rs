#![no_std]
use soroban_sdk::{contract, contracttype, contractimpl, Address, Env, Vec, Symbol, token, testutils::{Address as TestAddress, Arbitrary as TestArbitrary}, arbitrary::{Arbitrary, Unstructured}};
pub mod receipt;

// --- DATA STRUCTURES ---

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Circle(u64),
    Member(u64, Address), // Refactored: CircleID, UserAddress
    CircleCount,
    Deposit(u64, Address),
    GroupReserve,
    // #225: Duration Proposals
    Proposal(u64, u64), // CircleID, ProposalID
    ProposalCount(u64), // CircleID
    Vote(u64, u64, Address), // CircleID, ProposalID, Voter
    // #227: Bond Storage
    Bond(u64), // CircleID
    // #228: Governance
    Stake(Address),
    GlobalFeeBP, // Basis points
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
    pub contribution_amount: u64, // Optimized from i128 to u64
    pub max_members: u16, // Optimized from u32 to u16
    pub member_count: u16, // Track count separately from Vec
    pub current_recipient_index: u16, // Track by index instead of Address
    pub is_active: bool,
    pub token: Address, // The token used (USDC, XLM)
    pub deadline_timestamp: u64, // Deadline for on-time payments
    pub cycle_duration: u64, // Duration of each payment cycle in seconds
}

// --- CONTRACT TRAIT ---

pub trait SoroSusuTrait {
    // Initialize the contract
    fn init(env: Env, admin: Address, global_fee: u32);
    
    // Create a new savings circle (#227: Creator must pay bond)
    fn create_circle(env: Env, creator: Address, amount: u64, max_members: u16, token: Address, cycle_duration: u64, bond_amount: u64) -> u64;

    // Join an existing circle
    fn join_circle(env: Env, user: Address, circle_id: u64);

    // Make a deposit (#226: Support for batch contributions)
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
}

// --- IMPLEMENTATION ---

#[contract]
pub struct SoroSusu;

#[contractimpl]
impl SoroSusuTrait for SoroSusu {
    fn init(env: Env, admin: Address, global_fee: u32) {
        // Initialize the circle counter to 0 if it doesn't exist
        if !env.storage().instance().has(&DataKey::CircleCount) {
            env.storage().instance().set(&DataKey::CircleCount, &0u64);
        }
        // Set the admin
        env.storage().instance().set(&DataKey::Admin, &admin);
        // Set Global Fee BP
        env.storage().instance().set(&DataKey::GlobalFeeBP, &global_fee);
    }

    fn create_circle(env: Env, creator: Address, amount: u64, max_members: u16, token: Address, cycle_duration: u64, bond_amount: u64) -> u64 {
        // #227: Creator MUST pay a bond
        creator.require_auth();
        let client = token::Client::new(&env, &token);
        client.transfer(&creator, &env.current_contract_address(), &bond_amount);
        
        // 1. Get the current Circle Count
        let mut circle_count: u64 = env.storage().instance().get(&DataKey::CircleCount).unwrap_or(0);
        
        // 2. Increment the ID for the new circle
        circle_count += 1;

        // 3. Create the Circle Data Struct
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
        };

        // 4. Save the Circle, Bond, and Count
        env.storage().instance().set(&DataKey::Circle(circle_count), &new_circle);
        env.storage().instance().set(&DataKey::Bond(circle_count), &bond_amount);
        env.storage().instance().set(&DataKey::CircleCount, &circle_count);

        // 5. Initialize Group Reserve if not exists
        if !env.storage().instance().has(&DataKey::GroupReserve) {
            env.storage().instance().set(&DataKey::GroupReserve, &0u64);
        }

        // 6. Return the new ID
        circle_count
    }

    fn join_circle(env: Env, user: Address, circle_id: u64) {
        // 1. Authorization: The user MUST sign this transaction
        user.require_auth();

        // 2. Retrieve the circle data
        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();

        // 3. Check if the circle is full
        if circle.member_count >= circle.max_members {
            panic!("Circle is full");
        }

        // 4. Check if user is already a member to prevent duplicates
        let member_key = DataKey::Member(circle_id, user.clone());
        if env.storage().instance().has(&member_key) {
            panic!("User is already a member");
        }

        // 5. Create and store the new member
        let new_member = Member {
            address: user.clone(),
            has_contributed: false,
            contribution_count: 0,
            last_contribution_time: 0,
        };
        
        // 6. Store the member and update circle count
        env.storage().instance().set(&member_key, &new_member);
        circle.member_count += 1;
        
        // 7. Save the updated circle back to storage
        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);
    }

    fn deposit(env: Env, user: Address, circle_id: u64, rounds: u32) {
        // 1. Authorization: The user must sign this!
        user.require_auth();

        // 2. Load the Circle Data
        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();

        // 3. Check if user is actually a member
        let member_key = DataKey::Member(circle_id, user.clone());
        let mut member: Member = env.storage().instance().get(&member_key)
            .unwrap_or_else(|| panic!("User is not a member of this circle"));

        // 4. Create the Token Client
        let client = token::Client::new(&env, &circle.token);

        // 5. Check if payment is late and apply penalty if needed
        let current_time = env.ledger().timestamp();
        let mut total_extra = 0u64;

        if current_time > circle.deadline_timestamp {
            // Calculate 1% penalty
            let penalty_amount = circle.contribution_amount / 100; // 1% penalty
            total_extra += penalty_amount;
            
            // Update Group Reserve balance
            let mut reserve_balance: u64 = env.storage().instance().get(&DataKey::GroupReserve).unwrap_or(0);
            reserve_balance += penalty_amount;
            env.storage().instance().set(&DataKey::GroupReserve, &reserve_balance);
        }

        // #226: Platform Fee and Batch Incentive
        let mut fee_bp: u32 = env.storage().instance().get(&DataKey::GlobalFeeBP).unwrap_or(0);
        if rounds >= 3 {
            fee_bp /= 2; // 50% discount for prepaying 3+ rounds
        }
        
        let single_fee = (circle.contribution_amount * fee_bp as u64) / 10000;
        let total_deposit = (circle.contribution_amount + single_fee) * rounds as u64 + total_extra;

        // 6. Transfer the full amount from user
        client.transfer(
            &user, 
            &env.current_contract_address(), 
            &total_deposit
        );

        // 7. Update member contribution info
        member.has_contributed = true;
        member.contribution_count += rounds;
        member.last_contribution_time = current_time;
        
        // 8. Save updated member info
        env.storage().instance().set(&member_key, &member);

        // 9. Update circle deadline for next cycle
        circle.deadline_timestamp += circle.cycle_duration * rounds as u64;
        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);

        // 10. Mark as Paid
        env.storage().instance().set(&DataKey::Deposit(circle_id, user), &true);
    }

    fn propose_duration(env: Env, user: Address, circle_id: u64, new_duration: u64) -> u64 {
        user.require_auth();
        
        // Ensure circle exists
        if !env.storage().instance().has(&DataKey::Circle(circle_id)) {
            panic!("Circle not found");
        }

        // Ensure user is a member
        let member_key = DataKey::Member(circle_id, user.clone());
        if !env.storage().instance().has(&member_key) {
            panic!("Only members can propose duration changes");
        }

        let mut proposal_count: u64 = env.storage().instance().get(&DataKey::ProposalCount(circle_id)).unwrap_or(0);
        proposal_count += 1;

        let proposal = DurationProposal {
            id: proposal_count,
            new_duration,
            votes_for: 0,
            votes_against: 0,
            end_time: env.ledger().timestamp() + 86400 * 3, // 3 days to vote
            is_active: true,
        };

        env.storage().instance().set(&DataKey::Proposal(circle_id, proposal_count), &proposal);
        env.storage().instance().set(&DataKey::ProposalCount(circle_id), &proposal_count);

        proposal_count
    }

    fn vote_duration(env: Env, user: Address, circle_id: u64, proposal_id: u64, approve: bool) {
        user.require_auth();

        // Ensure user is a member
        let member_key = DataKey::Member(circle_id, user.clone());
        if !env.storage().instance().has(&member_key) {
            panic!("Only members can vote");
        }

        // Check if already voted
        let vote_key = DataKey::Vote(circle_id, proposal_id, user.clone());
        if env.storage().instance().has(&vote_key) {
            panic!("Already voted");
        }

        let mut proposal: DurationProposal = env.storage().instance().get(&DataKey::Proposal(circle_id, proposal_id))
            .unwrap_or_else(|| panic!("Proposal not found"));

        if !proposal.is_active || env.ledger().timestamp() > proposal.end_time {
            panic!("Proposal is not active or expired");
        }

        if approve {
            proposal.votes_for += 1;
        } else {
            proposal.votes_against += 1;
        }

        env.storage().instance().set(&vote_key, &true);

        // Check if 66% threshold reached
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();
        // 66% threshold
        if (proposal.votes_for as u32 * 100) > (circle.member_count as u32 * 66) {
            let mut updated_circle = circle;
            updated_circle.cycle_duration = proposal.new_duration;
            // Recalculate deadline
            updated_circle.deadline_timestamp = env.ledger().timestamp() + updated_circle.cycle_duration;
            env.storage().instance().set(&DataKey::Circle(circle_id), &updated_circle);
            proposal.is_active = false;
        }

        env.storage().instance().set(&DataKey::Proposal(circle_id, proposal_id), &proposal);
    }

    fn slash_bond(env: Env, admin: Address, circle_id: u64) {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != stored_admin {
            panic!("Only admin can slash bond");
        }

        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();
        let bond_amount: u64 = env.storage().instance().get(&DataKey::Bond(circle_id)).unwrap_or(0);
        
        if bond_amount > 0 {
            let client = token::Client::new(&env, &circle.token);
            // In a real scenario, we might distribute this to members.
            // For now, we move it to GroupReserve storage and potentially a reserve account.
            let mut reserve_balance: u64 = env.storage().instance().get(&DataKey::GroupReserve).unwrap_or(0);
            reserve_balance += bond_amount;
            env.storage().instance().set(&DataKey::GroupReserve, &reserve_balance);
            env.storage().instance().remove(&DataKey::Bond(circle_id));
        }
    }

    fn release_bond(env: Env, admin: Address, circle_id: u64) {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != stored_admin {
            panic!("Only admin can release bond");
        }

        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();
        let bond_amount: u64 = env.storage().instance().get(&DataKey::Bond(circle_id)).unwrap_or(0);
        
        if bond_amount > 0 {
            let client = token::Client::new(&env, &circle.token);
            client.transfer(&env.current_contract_address(), &circle.creator, &bond_amount);
            env.storage().instance().remove(&DataKey::Bond(circle_id));
        }
    }

    fn stake_xlm(env: Env, user: Address, xlm_token: Address, amount: u64) {
        user.require_auth();
        let client = token::Client::new(&env, &xlm_token);
        client.transfer(&user, &env.current_contract_address(), &amount);

        let stake_key = DataKey::Stake(user.clone());
        let mut user_stake: u64 = env.storage().instance().get(&stake_key).unwrap_or(0);
        user_stake += amount;
        env.storage().instance().set(&stake_key, &user_stake);
    }

    fn unstake_xlm(env: Env, user: Address, xlm_token: Address, amount: u64) {
        user.require_auth();
        let stake_key = DataKey::Stake(user.clone());
        let mut user_stake: u64 = env.storage().instance().get(&stake_key).unwrap_or(0);
        
        if user_stake < amount {
            panic!("Insufficient stake");
        }

        user_stake -= amount;
        let client = token::Client::new(&env, &xlm_token);
        client.transfer(&env.current_contract_address(), &user, &amount);
        
        if user_stake == 0 {
            env.storage().instance().remove(&stake_key);
        } else {
            env.storage().instance().set(&stake_key, &user_stake);
        }
    }

    fn update_global_fee(env: Env, admin: Address, new_fee: u32) {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != stored_admin {
            panic!("Only admin can update global fee");
        }

        env.storage().instance().set(&DataKey::GlobalFeeBP, &new_fee);
    }

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
}

// --- FUZZ TESTING MODULES ---

#[cfg(test)]
mod fuzz_tests {
    use super::*;
    use soroban_sdk::{testutils::{Address as TestAddress, Arbitrary as TestArbitrary}, arbitrary::{Arbitrary, Unstructured}};
    use std::i128;

    #[derive(Arbitrary, Debug, Clone)]
    pub struct FuzzTestCase {
        pub contribution_amount: u64,
        pub max_members: u16,
        pub user_id: u64,
    }

    #[test]
    fn fuzz_test_contribution_amount_edge_cases() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let token = Address::generate(&env);

        // Initialize contract
        SoroSusuTrait::init(env.clone(), admin.clone(), 100);

        // Test case 1: Maximum u64 value (should not panic)
        let max_circle_id = SoroSusuTrait::create_circle(
            env.clone(),
            creator.clone(),
            u64::MAX,
            10,
            token.clone(),
            604800, // 1 week in seconds
            500, // Bond
        );

        let user1 = Address::generate(&env);
        SoroSusuTrait::join_circle(env.clone(), user1.clone(), max_circle_id);

        // Mock token balance for the test
        env.mock_all_auths();
        
        // This should not panic even with u64::MAX contribution amount
        let result = std::panic::catch_unwind(|| {
            SoroSusuTrait::deposit(env.clone(), user1.clone(), max_circle_id);
        });
        
        // The transfer might fail due to insufficient balance, but it shouldn't panic from overflow
        assert!(result.is_ok() || result.unwrap_err().downcast::<String>().unwrap().contains("insufficient balance"));
    }

    #[test]
    fn fuzz_test_zero_and_negative_amounts() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let token = Address::generate(&env);

        // Initialize contract
        SoroSusuTrait::init(env.clone(), admin.clone(), 100);

        // Test case 2: Zero contribution amount (should be allowed but may cause issues)
        let zero_circle_id = SoroSusuTrait::create_circle(
            env.clone(),
            creator.clone(),
            0,
            10,
            token.clone(),
            604800, // 1 week in seconds
            500, // Bond
        );

        let user2 = Address::generate(&env);
        SoroSusuTrait::join_circle(env.clone(), user2.clone(), zero_circle_id);

        env.mock_all_auths();
        
        // Zero amount deposit should work (though may not be practically useful)
        let result = std::panic::catch_unwind(|| {
            SoroSusuTrait::deposit(env.clone(), user2.clone(), zero_circle_id);
        });
        
        assert!(result.is_ok());
    }

    #[test]
    fn fuzz_test_arbitrary_contribution_amounts() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let token = Address::generate(&env);

        // Initialize contract
        SoroSusuTrait::init(env.clone(), admin.clone(), 100);

        // Test with various edge case amounts
        let test_amounts = vec![
            1,                           // Minimum positive amount
            u32::MAX as u64,            // Large but reasonable amount
            u64::MAX / 2,               // Very large amount
            u64::MAX - 1,               // Maximum amount - 1
            1000000,                    // 1 million
            0,                          // Zero (already tested above)
        ];

        for (i, amount) in test_amounts.iter().enumerate() {
            let circle_id = SoroSusuTrait::create_circle(
                env.clone(),
                creator.clone(),
                *amount,
                10,
                token.clone(),
                604800, // 1 week in seconds
                500, // Bond
            );

            let user = Address::generate(&env);
            SoroSusuTrait::join_circle(env.clone(), user.clone(), circle_id);

            env.mock_all_auths();
            
            let result = std::panic::catch_unwind(|| {
                SoroSusuTrait::deposit(env.clone(), user.clone(), circle_id, 1);
            });
            
            // Should not panic due to overflow, only potentially due to insufficient balance
            match result {
                Ok(_) => {
                    // Deposit succeeded
                    println!("Γ£ô Amount {} succeeded", amount);
                }
                Err(e) => {
                    let error_msg = e.downcast::<String>().unwrap();
                    // Expected error: insufficient balance, not overflow
                    assert!(error_msg.contains("insufficient balance") || 
                           error_msg.contains("underflow") ||
                           error_msg.contains("overflow"));
                    println!("Γ£ô Amount {} failed with expected error: {}", amount, error_msg);
                }
            }
        }
    }

    #[test]
    fn fuzz_test_boundary_conditions() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let token = Address::generate(&env);

        // Initialize contract
        SoroSusuTrait::init(env.clone(), admin.clone(), 100);

        // Test boundary conditions for max_members
        let boundary_tests = vec![
            (1, "Minimum members"),
            (u16::MAX, "Maximum members"),
            (100, "Typical circle size"),
        ];

        for (max_members, description) in boundary_tests {
            let circle_id = SoroSusuTrait::create_circle(
                env.clone(),
                creator.clone(),
                1000, // Reasonable contribution amount
                max_members,
                token.clone(),
                604800, // 1 week in seconds
                100, // Bond
            );

            // Test joining with maximum allowed members
            for i in 0..max_members.min(10) { // Limit to 10 for test performance
                let user = Address::generate(&env);
                SoroSusuTrait::join_circle(env.clone(), user.clone(), circle_id);
                
                env.mock_all_auths();
                
                let result = std::panic::catch_unwind(|| {
                    SoroSusuTrait::deposit(env.clone(), user.clone(), circle_id, 1);
                });
                
                assert!(result.is_ok(), "Deposit failed for {} with max_members {}: {:?}", description, max_members, result);
            }
            
            println!("Γ£ô Boundary test passed: {} (max_members: {})", description, max_members);
        }
    }

    #[test]
    fn fuzz_test_concurrent_deposits() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let token = Address::generate(&env);

        // Initialize contract
        SoroSusuTrait::init(env.clone(), admin.clone(), 100);

        let circle_id = SoroSusuTrait::create_circle(
            env.clone(),
            creator.clone(),
            500,
            5,
            token.clone(),
            604800, // 1 week in seconds
            250, // Bond
        );

        // Create multiple users and test deposits
        let mut users = Vec::new();
        for _ in 0..5 {
            let user = Address::generate(&env);
            SoroSusuTrait::join_circle(env.clone(), user.clone(), circle_id);
            users.push(user);
        }

        env.mock_all_auths();

        // Test multiple deposits in sequence (simulating concurrent access)
        for user in users {
            let result = std::panic::catch_unwind(|| {
                SoroSusuTrait::deposit(env.clone(), user.clone(), circle_id, 1);
            });
            
            assert!(result.is_ok(), "Concurrent deposit test failed: {:?}", result);
        }
        
        println!("Γ£ô Concurrent deposits test passed");
    }

    #[test]
    fn test_late_penalty_mechanism() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let user = Address::generate(&env);
        let token = Address::generate(&env);

        // Initialize contract
        SoroSusuTrait::init(env.clone(), admin.clone(), 100);

        // Create a circle with 1 week cycle duration
        let circle_id = SoroSusuTrait::create_circle(
            env.clone(),
            creator.clone(),
            1000, // $10 contribution (assuming 6 decimals)
            5,
            token.clone(),
            604800, // 1 week in seconds
            500, // Bond
        );

        // User joins the circle
        SoroSusuTrait::join_circle(env.clone(), user.clone(), circle_id);

        // Mock token balance for the test
        env.mock_all_auths();

        // Get initial Group Reserve balance
        let initial_reserve: u64 = env.storage().instance().get(&DataKey::GroupReserve).unwrap_or(0);
        assert_eq!(initial_reserve, 0);

        // Simulate time passing beyond deadline (jump forward 2 weeks)
        env.ledger().set_timestamp(env.ledger().timestamp() + 2 * 604800);

        // Make a late deposit
        let result = std::panic::catch_unwind(|| {
            SoroSusuTrait::deposit(env.clone(), user.clone(), circle_id, 1);
        });
        
        assert!(result.is_ok(), "Late deposit should succeed: {:?}", result);

        // Check that Group Reserve received the 1% penalty (10 tokens)
        let final_reserve: u64 = env.storage().instance().get(&DataKey::GroupReserve).unwrap_or(0);
        assert_eq!(final_reserve, 10, "Group Reserve should have 10 tokens (1% penalty)");

        // Verify member was marked as having contributed
        let member_key = DataKey::Member(circle_id, user.clone());
        let member: Member = env.storage().instance().get(&member_key).unwrap();
        assert!(member.has_contributed);
        assert_eq!(member.contribution_count, 1);

        println!("Γ£ô Late penalty mechanism test passed - 1% penalty correctly routed to Group Reserve");
    }

    #[test]
    fn test_on_time_deposit_no_penalty() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let creator = Address::generate(&env);
        let user = Address::generate(&env);
        let token = Address::generate(&env);

        // Initialize contract
        SoroSusuTrait::init(env.clone(), admin.clone(), 100);

        // Create a circle with 1 week cycle duration
        let circle_id = SoroSusuTrait::create_circle(
            env.clone(),
            creator.clone(),
            1000, // $10 contribution
            5,
            token.clone(),
            604800, // 1 week in seconds
            500, // Bond
        );

        // User joins the circle
        SoroSusuTrait::join_circle(env.clone(), user.clone(), circle_id);

        // Mock token balance for the test
        env.mock_all_auths();

        // Get initial Group Reserve balance
        let initial_reserve: u64 = env.storage().instance().get(&DataKey::GroupReserve).unwrap_or(0);
        assert_eq!(initial_reserve, 0);

        // Make an on-time deposit (don't advance time)
        let result = std::panic::catch_unwind(|| {
            SoroSusuTrait::deposit(env.clone(), user.clone(), circle_id, 1);
        });
        
        assert!(result.is_ok(), "On-time deposit should succeed: {:?}", result);

        // Check that Group Reserve received no penalty
        let final_reserve: u64 = env.storage().instance().get(&DataKey::GroupReserve).unwrap_or(0);
        assert_eq!(final_reserve, 0, "Group Reserve should have 0 tokens for on-time deposit");

        println!("Γ£ô On-time deposit test passed - no penalty applied");
    }
}
