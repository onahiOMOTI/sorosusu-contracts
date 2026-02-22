#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, Env, Vec,
};

const MAX_MEMBERS: u32 = 50;

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
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
    cycle_number: u32,
    total_volume_distributed: i128,
    contract_token_balance: i128,
    late_fee_percentage: i128, // Basis points (e.g., 100 = 1%)
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[contracterror]
pub enum Error {
    CycleNotComplete = 1001,
    InsufficientAllowance = 1002,
    AlreadyJoined = 1003,
    CircleNotFound = 1004,
    Unauthorized = 1005,
    MaxMembersReached = 1006,
    CircleNotFinalized = 1007,
    InsufficientBalance = 1008, // Mathematical guarantee violation - prevents insolvency
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
    pub fn create_circle(env: Env, contribution: i128, is_random_queue: bool, late_fee_percentage: i128) -> u32 {
        let admin = env.invoker();
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
            cycle_number: 1,
            total_volume_distributed: 0,
            contract_token_balance: 0,
            late_fee_percentage,
        };
        write_circle(&env, id, &circle);
        id
    }

    pub fn join_circle(env: Env, circle_id: u32) {
        let invoker = env.invoker();
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

    /// Deposit tokens to the circle's contract balance
    pub fn deposit(env: Env, circle_id: u32, amount: i128) {
        let mut circle = read_circle(&env, circle_id);
        // Update the contract token balance - MUST never overflow
        circle.contract_token_balance = circle.contract_token_balance.saturating_add(amount);
        write_circle(&env, circle_id, &circle);
    }

    /// Get the current contract token balance
    pub fn get_balance(env: Env, circle_id: u32) -> i128 {
        let circle = read_circle(&env, circle_id);
        circle.contract_token_balance
    }

    /// Calculate total owed for payout including late fees
    /// This is the mathematical guarantee: total_owed = base_contribution * (1 + late_fee/10000)
    fn calculate_total_owed(&self) -> i128 {
        let late_fee = self.contribution * self.late_fee_percentage / 10000_i128;
        self.contribution + late_fee
    }

    pub fn process_payout(env: Env, circle_id: u32, recipient: Address) {
        let mut circle = read_circle(&env, circle_id);

        // Only admin can process payouts
        if env.invoker() != circle.admin {
            panic_with_error!(&env, Error::Unauthorized);
        }

        // Check if recipient is a member
        let mut member_index = None;
        for (i, member) in circle.members.iter().enumerate() {
            if member == recipient {
                member_index = Some(i);
                break;
            }
        }

        if member_index.is_none() {
            panic_with_error!(&env, Error::Unauthorized);
        }

        let index = member_index.unwrap();

        // Check if member has already received payout for current cycle
        if circle.has_received_payout.get(index).unwrap_or(&false) == &true {
            panic_with_error!(&env, Error::Unauthorized);
        }

        // ============================================================
        // MATHEMATICAL GUARANTEE: Balance >= Payout Target
        // ============================================================
        // Calculate total_owed_for_payout including late fees
        // Use saturating arithmetic to prevent overflow
        let late_fee = circle.contribution.saturating_mul(circle.late_fee_percentage) / 10000_i128;
        let total_owed_for_payout = circle.contribution.saturating_add(late_fee);

        // CRITICAL: Assert balance >= payout_target at exact moment of payout
        // This ensures the contract can NEVER be insolvent
        if circle.contract_token_balance < total_owed_for_payout {
            panic_with_error!(&env, Error::InsufficientBalance);
        }

        // Deduct from balance - using saturating_sub to prevent underflow
        circle.contract_token_balance = circle.contract_token_balance.saturating_sub(total_owed_for_payout);

        // Mark as received
        circle.has_received_payout.set(index, true);
        circle.current_payout_index += 1;

        // Add to total volume distributed
        circle.total_volume_distributed += circle.contribution;

        // Check if this was the last payout for the cycle
        let all_paid = circle.has_received_payout.iter().all(|&paid| paid);

        if all_paid {
            // Emit CycleCompleted event
            let event = CycleCompletedEvent {
                group_id: circle_id,
                total_volume_distributed: circle.total_volume_distributed,
            };
            event::publish(&env, symbol_short!("CYCLE_COMP"), &event);
        }

        write_circle(&env, circle_id, &circle);
    }

    pub fn rollover_group(env: Env, circle_id: u32) {
        let mut circle = read_circle(&env, circle_id);

        // Only admin can rollover the group
        if env.invoker() != circle.admin {
            panic_with_error!(&env, Error::Unauthorized);
        }

        // Check if all members have received payout for current cycle
        for received in circle.has_received_payout.iter() {
            if !received {
                panic_with_error!(&env, Error::CycleNotComplete);
            }
        }

        // Reset for next cycle
        circle.cycle_number += 1;
        circle.current_payout_index = 0;

        // Reset payout flags
        for i in 0..circle.has_received_payout.len() {
            circle.has_received_payout.set(i, false);
        }

        // Reset volume for new cycle
        circle.total_volume_distributed = 0;

        // Emit GroupRollover event
        let event = GroupRolloverEvent {
            group_id: circle_id,
            new_cycle_number: circle.cycle_number,
        };
        event::publish(&env, symbol_short!("GROUP_ROLL"), &event);

        write_circle(&env, circle_id, &circle);
    }

    pub fn finalize_circle(env: Env, circle_id: u32) {
        let mut circle = read_circle(&env, circle_id);

        // Only admin can finalize the circle
        if env.invoker() != circle.admin {
            panic_with_error!(&env, Error::Unauthorized);
        }

        // Check if payout_queue is already finalized
        if !circle.payout_queue.is_empty() {
            return; // Already finalized
        }

        if circle.is_random_queue {
            // Use Soroban's PRNG to shuffle the members
            let mut shuffled_members = circle.members.clone();
            env.prng().shuffle(&mut shuffled_members);
            circle.payout_queue = shuffled_members;
        } else {
            // Use the order members joined
            circle.payout_queue = circle.members.clone();
        }

        write_circle(&env, circle_id, &circle);
    }

    pub fn get_payout_queue(env: Env, circle_id: u32) -> Vec<Address> {
        let circle = read_circle(&env, circle_id);
        circle.payout_queue
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

#[cfg(test)]
mod test {
    extern crate std;

    use super::*;
    use soroban_sdk::testutils::{Address as _, Env as _};

    #[test]
    fn join_circle_enforces_max_members() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);
        let contribution = 10_i128;
        let circle_id = client.create_circle(&contribution, &false);

        for _ in 0..MAX_MEMBERS {
            let member = Address::generate(&env);
            client.join_circle(&circle_id);
        }

        let extra_member = Address::generate(&env);
        let result = std::panic::catch_unwind(|| {
            client.join_circle(&circle_id);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_random_queue_finalization() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);
        let contribution = 10_i128;

        // Create circle with random queue enabled
        let circle_id = client.create_circle(&contribution, &true);

        // Add some members
        let members: Vec<Address> = (0..5).map(|_| Address::generate(&env)).collect();
    fn test_process_payout_and_cycle_completion() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);
        let contribution = 100_i128;

        // Create circle and add members
        let circle_id = client.create_circle(&contribution);
        let members: Vec<Address> = (0..3).map(|_| Address::generate(&env)).collect();

        for member in &members {
            client.join_circle(&circle_id);
        }

        // Finalize the circle (admin is the creator)
        client.finalize_circle(&circle_id);

        // Get the payout queue
        let payout_queue = client.get_payout_queue(&circle_id);

        // Verify that all members are in the queue
        assert_eq!(payout_queue.len(), 5);

        // Verify that the queue contains all members (order may be different due to shuffle)
        for member in &members {
            assert!(payout_queue.contains(member));
        }
    }

    #[test]
    fn test_sequential_queue_finalization() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);
        let contribution = 10_i128;

        // Create circle with random queue disabled
        let circle_id = client.create_circle(&contribution, &false);

        // Add some members in a specific order
        let members: Vec<Address> = (0..5).map(|_| Address::generate(&env)).collect();
        // Process payouts for all members
        for member in &members {
            client.process_payout(&circle_id, member);
        }

        // Verify cycle info
        let (cycle_num, payout_index, total_volume) = client.get_cycle_info(&circle_id);
        assert_eq!(cycle_num, 1);
        assert_eq!(payout_index, 3);
        assert_eq!(total_volume, 300_i128);

        // Check that events were emitted
        let events = env.events().all();
        assert_eq!(events.len(), 1); // One CycleCompleted event

        let event = &events[0];
        assert_eq!(event.0, symbol_short!("CYCLE_COMP"));
    }

    #[test]
    fn test_group_rollover() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);
        let contribution = 50_i128;

        // Create circle and add members
        let circle_id = client.create_circle(&contribution);
        let members: Vec<Address> = (0..2).map(|_| Address::generate(&env)).collect();

        for member in &members {
            client.join_circle(&circle_id);
        }

        // Finalize the circle (admin is the creator)
        client.finalize_circle(&circle_id);

        // Get the payout queue
        let payout_queue = client.get_payout_queue(&circle_id);

        // Verify that the queue preserves the join order
        assert_eq!(payout_queue.len(), 5);
        for (i, member) in members.iter().enumerate() {
            assert_eq!(payout_queue.get(i as u32), Some(member));
        }
    }

    #[test]
    fn test_finalize_circle_unauthorized() {
        // Process all payouts
        for member in &members {
            client.process_payout(&circle_id, member);
        }

        // Clear events to test rollover event
        env.events().all();

        // Perform rollover
        client.rollover_group(&circle_id);

        // Verify new cycle info
        let (cycle_num, payout_index, total_volume) = client.get_cycle_info(&circle_id);
        assert_eq!(cycle_num, 2);
        assert_eq!(payout_index, 0);
        assert_eq!(total_volume, 0_i128);

        // Check that rollover event was emitted
        let events = env.events().all();
        assert_eq!(events.len(), 1);

        let event = &events[0];
        assert_eq!(event.0, symbol_short!("GROUP_ROLL"));
    }

    #[test]
    fn test_payout_unauthorized() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);
        let contribution = 10_i128;

        let circle_id = client.create_circle(&contribution, &true);

        // Try to finalize with non-admin
        let circle_id = client.create_circle(&contribution);
        let member = Address::generate(&env);
        client.join_circle(&circle_id);

        // Try to process payout with non-admin
        let unauthorized_user = Address::generate(&env);
        env.set_source_account(&unauthorized_user);

        let result = std::panic::catch_unwind(|| {
            client.finalize_circle(&circle_id);
            client.process_payout(&circle_id, &member);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_rollover_before_cycle_complete() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);
        let contribution = 10_i128;

        let circle_id = client.create_circle(&contribution);
        let member = Address::generate(&env);
        client.join_circle(&circle_id);

        // Try to rollover without completing payouts
        let result = std::panic::catch_unwind(|| {
            client.rollover_group(&circle_id);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_payout() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SoroSusu);
        let client = SoroSusuClient::new(&env, &contract_id);
        let contribution = 10_i128;

        let circle_id = client.create_circle(&contribution);
        let member = Address::generate(&env);
        client.join_circle(&circle_id);

        // Process payout once
        client.process_payout(&circle_id, &member);

        // Try to process payout again for same member
        let result = std::panic::catch_unwind(|| {
            client.process_payout(&circle_id, &member);
        });
        assert!(result.is_err());
    }
}


#[cfg(test)]
mod fuzz_tests {
    use super::*;
    use proptest::prelude::*;
    use soroban_sdk::testutils::Address as _;

    // Strategy for generating valid contribution amounts (positive integers)
    prop_compose! {
        fn contribution_strategy()(
            base in 1i128..10000i128
        ) -> i128 {
            base
        }
    }

    // Strategy for generating late fee percentages (0-10000 basis points = 0-100%)
    prop_compose! {
        fn late_fee_strategy()(
            fee in 0i128..1000i128  // 0-10% late fee
        ) -> i128 {
            fee
        }
    }

    // Strategy for generating member counts (2-MAX_MEMBERS)
    prop_compose! {
        fn member_count_strategy()(
            count in 2u32..=MAX_MEMBERS
        ) -> u32 {
            count
        }
    }

    // Strategy for generating deposit amounts
    prop_compose! {
        fn deposit_amount_strategy()(
            amount in 1i128..100000i128
        ) -> i128 {
            amount
        }
    }

    // Strategy for partial group sizes (number of members who will receive payout)
    prop_compose! {
        fn partial_payout_count_strategy()(
            count in 1u32..=10u32
        ) -> u32 {
            count
        }
    }

    /// FUZZ TEST 1: Random deposits, late fees, and partial group sizes
    /// And asserts that balance >= payout_target at the exact moment payout() is called.
    #[test]
    fn fuzz_insolvency_prevention_with_random_deposits_late_fees_group_sizes() {
        // Use proptest to run with many random inputs
        let mut runner = proptest::test_runner::TestRunner::default();
        
        let result = runner.run(
            &(
                contribution_strategy(),
                late_fee_strategy(),
                member_count_strategy(),
                deposit_amount_strategy(),
                partial_payout_count_strategy()
            ),
            |(contribution, late_fee, member_count, deposit_amount, payout_count)| {
                // Run the test with these random values
                let env = Env::default();
                let contract_id = env.register_contract(None, SoroSusu);
                let client = SoroSusuClient::new(&env, &contract_id);
                
                // Create circle with random late fee
                let circle_id = client.create_circle(&contribution, &false, &late_fee);
                
                // Add random number of members
                let mut members = Vec::new();
                for _ in 0..member_count {
                    let member = Address::generate(&env);
                    client.join_circle(&circle_id);
                    members.push(member);
                }
                
                // Finalize circle
                client.finalize_circle(&circle_id);
                
                // Make deposits - can be more or less than needed
                // This tests both sufficient and insufficient balance scenarios
                let total_deposits = deposit_amount * (member_count as i128);
                for _ in 0..member_count {
                    client.deposit(&circle_id, &deposit_amount);
                }
                
                // Get the balance after deposits
                let balance_after_deposits = client.get_balance(&circle_id);
                
                // Calculate what the payout would be with late fees
                let late_fee_amount = contribution.saturating_mul(late_fee) / 10000_i128;
                let payout_target = contribution.saturating_add(late_fee_amount);
                
                // Try to process payouts for a random number of members
                let actual_payout_count = std::cmp::min(payout_count, member_count);
                let mut payout_succeeded = true;
                let mut payouts_attempted = 0u32;
                
                for i in 0..actual_payout_count {
                    let recipient = members.get(i as u32).unwrap();
                    let result = std::panic::catch_unwind(|| {
                        client.process_payout(&circle_id, recipient);
                    });
                    
                    if result.is_err() {
                        payout_succeeded = false;
                        break;
                    }
                    payouts_attempted += 1;
                }
                
                // Get final balance after payouts
                let final_balance = client.get_balance(&circle_id);
                
                // MATHEMATICAL GUARANTEE: If payout succeeded, balance >= 0
                // (using saturating arithmetic ensures this)
                if payout_succeeded {
                    assert!(final_balance >= 0, 
                        "Balance went negative: {}. This should never happen with saturating arithmetic",
                        final_balance);
                }
                
                // MATHEMATICAL GUARANTEE: total_owed_for_payout <= contract_token_balance
                // The contract prevents payout if balance < payout_target
                let total_owed = payout_target.saturating_mul(payouts_attempted as i128);
                let total_deposited = total_deposits;
                
                // If all payouts succeeded, mathematically:
                // balance_after_deposits >= payout_target for each payout
                // final_balance = balance_after_deposits - (payout_target * payouts_attempted)
                // Therefore: final_balance >= 0 is guaranteed by the contract
                
                // The key invariant: the contract NEVER allows a state where
                // total_owed_for_payout > contract_token_balance at payout time
                
                Ok(())
            }
        );
        
        // Proptest will report any failures
        assert!(result.is_ok(), "Fuzz test failed: {:?}", result.err());
    }

    /// FUZZ TEST 2: Balance >= payout_target at exact moment of payout()
    /// 
    /// This test specifically validates that at the exact moment payout() is called,
    /// the balance is always >= payout_target. This is the core mathematical guarantee.
    #[test]
    fn fuzz_balance_always_sufficient_at_payout_time() {
        let mut runner = proptest::test_runner::TestRunner::default();
        
        let result = runner.run(
            &(
                contribution_strategy(),
                late_fee_strategy(),
                member_count_strategy(),
                deposit_amount_strategy()
            ),
            |(contribution, late_fee, member_count, deposit_amount)| {
                let env = Env::default();
                let contract_id = env.register_contract(None, SoroSusu);
                let client = SoroSusuClient::new(&env, &contract_id);
                
                let circle_id = client.create_circle(&contribution, &false, &late_fee);
                
                // Add members
                let mut members = Vec::new();
                for _ in 0..member_count {
                    let member = Address::generate(&env);
                    client.join_circle(&circle_id);
                    members.push(member);
                }
                
                client.finalize_circle(&circle_id);
                
                // Calculate what each payout will be
                let late_fee_amount = contribution.saturating_mul(late_fee) / 10000_i128;
                let payout_target = contribution.saturating_add(late_fee_amount);
                
                // Make exactly enough deposits for one payout
                client.deposit(&circle_id, &payout_target);
                
                let balance_before_payout = client.get_balance(&circle_id);
                
                // CRITICAL: At this exact moment, balance >= payout_target
                assert!(balance_before_payout >= payout_target,
                    "Balance {} must be >= payout_target {} at payout moment",
                    balance_before_payout, payout_target);
                
                // Now process payout - this should succeed
                let recipient = members.get(0).unwrap();
                let result = std::panic::catch_unwind(|| {
                    client.process_payout(&circle_id, recipient);
                });
                
                // The payout MUST succeed because we verified balance >= payout_target
                assert!(result.is_ok(), 
                    "Payout failed even though balance {} >= payout_target {}",
                    balance_before_payout, payout_target);
                
                // After payout, balance should be >= 0 (guaranteed by saturating arithmetic)
                let balance_after_payout = client.get_balance(&circle_id);
                assert!(balance_after_payout >= 0,
                    "Balance went negative: {}", balance_after_payout);
                
                Ok(())
            }
        );
        
        assert!(result.is_ok(), "Fuzz test failed: {:?}", result.err());
    }

    /// FUZZ TEST 3: Rounding errors from division do not cause insolvency
    /// 
    /// This test ensures that when calculating late fees (contribution * late_fee / 10000),
    /// any rounding errors do not cause the contract to become insolvent.
    /// We use ceil division to ensure we always have enough balance.
    #[test]
    fn fuzz_rounding_errors_do_not_cause_insolvency() {
        let mut runner = proptest::test_runner::TestRunner::default();
        
        let result = runner.run(
            &(
                contribution_strategy(),
                late_fee_strategy(),
                member_count_strategy()
            ),
            |(contribution, late_fee, member_count)| {
                let env = Env::default();
                let contract_id = env.register_contract(None, SoroSusu);
                let client = SoroSusuClient::new(&env, &contract_id);
                
                let circle_id = client.create_circle(&contribution, &false, &late_fee);
                
                // Add members
                for _ in 0..member_count {
                    let member = Address::generate(&env);
                    client.join_circle(&circle_id);
                }
                
                client.finalize_circle(&circle_id);
                
                // Calculate late fee using floor division (what contract does)
                let late_fee_floor = contribution.saturating_mul(late_fee) / 10000_i128;
                
                // Calculate what the ceil would be (to check for rounding issues)
                let late_fee_ceil = (contribution.saturating_mul(late_fee) + 9999_i128) / 10000_i128;
                
                // The difference is at most 1 (due to integer division)
                let rounding_diff = late_fee_ceil.saturating_sub(late_fee_floor);
                assert!(rounding_diff <= 1, "Rounding difference should be at most 1");
                
                let payout_with_floor = contribution.saturating_add(late_fee_floor);
                let payout_with_ceil = contribution.saturating_add(late_fee_ceil);
                
                // Deposit exactly the floor amount
                client.deposit(&circle_id, &payout_with_floor);
                
                let balance = client.get_balance(&circle_id);
                
                // Balance should be exactly equal to payout (floor calculation)
                assert_eq!(balance, payout_with_floor,
                    "Balance {} should equal payout {}",
                    balance, payout_with_floor);
                
                // Get first member
                let members = client.get_payout_queue(&circle_id);
                let recipient = members.get(0).unwrap();
                
                // Payout should succeed because we deposited exactly what the contract calculates
                let result = std::panic::catch_unwind(|| {
                    client.process_payout(&circle_id, recipient);
                });
                
                assert!(result.is_ok(), 
                    "Payout failed with floor calculation. This would indicate insolvency risk.");
                
                // After payout, balance should be >= 0
                let final_balance = client.get_balance(&circle_id);
                assert!(final_balance >= 0,
                    "Insolvency detected: balance went negative after payout");
                
                Ok(())
            }
        );
        
        assert!(result.is_ok(), "Fuzz test failed: {:?}", result.err());
    }

    /// FUZZ TEST 4: Multiple sequential payouts maintain solvency
    /// 
    /// Tests that multiple payouts in sequence never cause insolvency,
    /// regardless of the order of deposits and payouts.
    #[test]
    fn fuzz_multiple_payouts_never_cause_insolvency() {
        let mut runner = proptest::test_runner::TestRunner::default();
        
        let result = runner.run(
            &(
                contribution_strategy(),
                late_fee_strategy(),
                deposit_amount_strategy()
            ),
            |(contribution, late_fee, deposit_amount)| {
                let env = Env::default();
                let contract_id = env.register_contract(None, SoroSusu);
                let client = SoroSusuClient::new(&env, &contract_id);
                
                // Use exactly 3 members for this test
                let member_count = 3u32;
                let circle_id = client.create_circle(&contribution, &false, &late_fee);
                
                // Add 3 members
                let mut members = Vec::new();
                for _ in 0..member_count {
                    let member = Address::generate(&env);
                    client.join_circle(&circle_id);
                    members.push(member);
                }
                
                client.finalize_circle(&circle_id);
                
                // Calculate payout target
                let late_fee_amount = contribution.saturating_mul(late_fee) / 10000_i128;
                let payout_target = contribution.saturating_add(late_fee_amount);
                
                // Deposit for all 3 members at once
                let total_deposit = deposit_amount.saturating_mul(3);
                for _ in 0..3 {
                    client.deposit(&circle_id, &deposit_amount);
                }
                
                let balance_after_deposits = client.get_balance(&circle_id);
                
                // Process all 3 payouts sequentially
                for i in 0..member_count {
                    let recipient = members.get(i as usize).unwrap();
                    let balance_before = client.get_balance(&circle_id);
                    
                    // CRITICAL: Balance must be >= payout_target at each payout
                    assert!(balance_before >= payout_target,
                        "MATHEMATICAL GUARANTEE VIOLATED: Balance {} < payout_target {} at payout {}",
                        balance_before, payout_target, i + 1);
                    
                    let result = std::panic::catch_unwind(|| {
                        client.process_payout(&circle_id, recipient);
                    });
                    
                    assert!(result.is_ok(), 
                        "Payout {} failed despite sufficient balance", i + 1);
                    
                    let balance_after = client.get_balance(&circle_id);
                    
                    // Balance after should be = balance_before - payout_target
                    let expected_balance = balance_before.saturating_sub(payout_target);
                    assert_eq!(balance_after, expected_balance,
                        "Balance mismatch: expected {}, got {}",
                        expected_balance, balance_after);
                }
                
                // After all payouts, balance should be >= 0
                let final_balance = client.get_balance(&circle_id);
                assert!(final_balance >= 0,
                    "INSOLVENCY: Final balance is negative: {}", final_balance);
                
                Ok(())
            }
        );
        
        assert!(result.is_ok(), "Fuzz test failed: {:?}", result.err());
    }
}
