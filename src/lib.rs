#![no_std]
use soroban_sdk::{contract, contracttype, contractimpl, Address, Env, Symbol, token};

// --- DATA STRUCTURES ---

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Circle(u64),
    Member(Address),
    CircleCount,
    // New: Tracks if a user has paid for a specific circle (CircleID, UserAddress)
    Deposit(u64, Address),
    // New: Tracks Group Reserve balance for penalties
    GroupReserve,
    // New: Tracks next cycle contribution amount for each circle
    NextCycleAmount(u64),
    // New: Tracks claimable balances for each user in each circle
    ClaimableBalance(u64, Address),
    // New: Tracks co-winners configuration for each circle
    CoWinnersConfig(u64),
    // New: Tracks current round winners for each circle
    CurrentWinners(u64),
    // New: Tracks user reputation score for tiered access
    UserReputation(Address),
    // New: Tracks private contribution amounts for privacy masking
    PrivateContribution(u64, Address),
    // New: Tracks voting proposals
    VotingProposal(u64),
    // New: Tracks votes on proposals
    Vote(u64, Address),
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
    pub contribution_amount: i128, // Changed back to i128 for token compatibility
    pub max_members: u32, // Changed from u16 to u32 for Soroban compatibility
    pub member_count: u32, // Changed from u16 to u32 for Soroban compatibility
    pub current_recipient_index: u32, // Changed from u16 to u32 for Soroban compatibility
    pub is_active: bool,
    pub token: Address, // The token used (USDC, XLM)
    pub deadline_timestamp: u64, // Deadline for on-time payments
    pub cycle_duration: u64, // Duration of each payment cycle in seconds
    // New: Fields for co-winners and tiered access
    pub max_co_winners: u32, // Maximum number of co-winners per round
    pub min_reputation_required: u64, // Minimum reputation score to join
}

// --- EVENTS ---

#[contracttype]
#[derive(Clone, Debug)]
pub struct AdminChangedEvent {
    pub old_admin: Address,
    pub new_admin: Address,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct CoWinnersConfig {
    pub enabled: bool,
    pub max_winners: u32,
    pub split_method: u32, // 0 = equal split, 1 = proportional to contributions
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct ContributionMaskedEvent {
    pub member_id: Address,
    pub success: bool,
    // Amount is NOT included for privacy
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct VotingProposal {
    pub id: u64,
    pub circle_id: u64,
    pub proposal_type: u32, // 0 = meeting date change, 1 = new member, 2 = other
    pub description: String,
    pub proposer: Address,
    pub created_at: u64,
    pub voting_deadline: u64,
    pub yes_votes: u64,
    pub no_votes: u64,
    pub total_voting_power: u64,
    pub is_executed: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct VoteCast {
    pub proposal_id: u64,
    pub voter: Address,
    pub vote: bool, // true = yes, false = no
    pub voting_power: u64,
}

// --- CONTRACT TRAIT ---

pub trait SoroSusuTrait {
    // Initialize the contract
    fn init(env: Env, admin: Address);
    
    // Create a new savings circle
    fn create_circle(env: Env, creator: Address, amount: i128, max_members: u32, token: Address, cycle_duration: u64, max_co_winners: u32, min_reputation_required: u64) -> u64;

    // Join an existing circle
    fn join_circle(env: Env, user: Address, circle_id: u64);

    // Make a deposit (Pay your weekly/monthly due)
    fn deposit(env: Env, user: Address, circle_id: u64, privacy_masked: bool);
    
    // Transfer admin role to another user
    fn transfer_admin(env: Env, current_admin: Address, new_admin: Address);
    
    // Set next cycle contribution amount (Admin only)
    fn set_next_cycle_amount(env: Env, admin: Address, circle_id: u64, amount: i128);
    
    // Configure co-winners for a circle (Admin only)
    fn configure_co_winners(env: Env, admin: Address, circle_id: u64, enabled: bool, max_winners: u32, split_method: u32);
    
    // Distribute funds to members with co-winners support (pull pattern)
    fn distribute_funds(env: Env, admin: Address, circle_id: u64, co_winners: Vec<Address>);
    
    // Claim funds from distribution
    fn claim(env: Env, user: Address, circle_id: u64);
    
    // Create a voting proposal
    fn create_proposal(env: Env, proposer: Address, circle_id: u64, proposal_type: u32, description: String, voting_deadline: u64) -> u64;
    
    // Vote on a proposal
    fn vote(env: Env, voter: Address, proposal_id: u64, vote: bool);
    
    // Execute a successful proposal
    fn execute_proposal(env: Env, executor: Address, proposal_id: u64);
    
    // Update user reputation (Admin only)
    fn update_reputation(env: Env, admin: Address, user: Address, reputation_score: u64);
    
    // Get private contribution amount (member only)
    fn get_private_contribution(env: Env, user: Address, circle_id: u64, target_member: Address) -> i128;
}

// --- IMPLEMENTATION ---

#[contract]
pub struct SoroSusu;

#[contractimpl]
impl SoroSusuTrait for SoroSusu {
    fn init(env: Env, admin: Address) {
        // Initialize the circle counter to 0 if it doesn't exist
        if !env.storage().instance().has(&DataKey::CircleCount) {
            env.storage().instance().set(&DataKey::CircleCount, &0u64);
        }
        // Set the admin
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    fn create_circle(env: Env, creator: Address, amount: i128, max_members: u32, token: Address, cycle_duration: u64, max_co_winners: u32, min_reputation_required: u64) -> u64 {
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
            max_co_winners,
            min_reputation_required,
        };

        // 4. Save the Circle and the new Count
        env.storage().instance().set(&DataKey::Circle(circle_count), &new_circle);
        env.storage().instance().set(&DataKey::CircleCount, &circle_count);

        // 5. Initialize Group Reserve if not exists
        if !env.storage().instance().has(&DataKey::GroupReserve) {
            env.storage().instance().set(&DataKey::GroupReserve, &0u64);
        }

        // 6. Initialize co-winners configuration
        let co_winners_config = CoWinnersConfig {
            enabled: max_co_winners > 1,
            max_winners: max_co_winners,
            split_method: 0, // Default to equal split
        };
        env.storage().instance().set(&DataKey::CoWinnersConfig(circle_count), &co_winners_config);

        // 7. Return the new ID
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
        let member_key = DataKey::Member(user.clone());
        if env.storage().instance().has(&member_key) {
            panic!("User is already a member");
        }

        // 5. ENHANCED: Check user reputation against circle requirements (Tiered Access)
        if circle.min_reputation_required > 0 {
            let user_reputation: u64 = env.storage().instance().get(&DataKey::UserReputation(user.clone())).unwrap_or(0);
            if user_reputation < circle.min_reputation_required {
                panic!("User reputation is too low to join this circle. Required: {}, Current: {}", circle.min_reputation_required, user_reputation);
            }
            
            // Emit reputation check event for transparency
            env.events().publish((Symbol::new(&env, "reputation_check"),), (circle_id, user, user_reputation, circle.min_reputation_required));
        }

        // 6. Create and store the new member
        let new_member = Member {
            address: user.clone(),
            has_contributed: false,
            contribution_count: 0,
            last_contribution_time: 0,
        };
        
        // 7. Store the member and update circle count
        env.storage().instance().set(&member_key, &new_member);
        circle.member_count += 1;
        
        // 8. Save the updated circle back to storage
        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);

        // 9. Emit member joined event
        env.events().publish((Symbol::new(&env, "member_joined"),), (circle_id, user, circle.member_count));
    }

    fn deposit(env: Env, user: Address, circle_id: u64, privacy_masked: bool) {
        // 1. Authorization: The user must sign this!
        user.require_auth();

        // 2. Load the Circle Data
        let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id)).unwrap();

        // 2.1. Check if there is a next cycle amount set
        let next_cycle_amount: Option<i128> = env.storage().instance().get(&DataKey::NextCycleAmount(circle_id));
        let contribution_amount = next_cycle_amount.unwrap_or(circle.contribution_amount);

        // 3. Check if user is actually a member
        let member_key = DataKey::Member(user.clone());
        let mut member: Member = env.storage().instance().get(&member_key)
            .unwrap_or_else(|| panic!("User is not a member of this circle"));

        // 4. Create the Token Client
        let client = token::Client::new(&env, &circle.token);

        // 5. Check if payment is late and apply penalty if needed
        let current_time = env.ledger().timestamp();
        let mut penalty_amount = 0i128;

        if current_time > circle.deadline_timestamp {
            // Calculate 1% penalty
            penalty_amount = contribution_amount / 100; // 1% penalty
            
            // Update Group Reserve balance
            let mut reserve_balance: u64 = env.storage().instance().get(&DataKey::GroupReserve).unwrap_or(0);
            reserve_balance += penalty_amount as u64;
            env.storage().instance().set(&DataKey::GroupReserve, &reserve_balance);
        }

        // 6. Transfer the full amount from user
        client.transfer(
            &user, 
            &env.current_contract_address(), 
            &contribution_amount
        );

        // 7. Store private contribution amount (ALWAYS store for privacy)
        let private_key = DataKey::PrivateContribution(circle_id, user.clone());
        env.storage().instance().set(&private_key, &contribution_amount);

        // 8. Update member contribution info
        member.has_contributed = true;
        member.contribution_count += 1;
        member.last_contribution_time = current_time;
        
        // 9. Save updated member info
        env.storage().instance().set(&member_key, &member);

        // 10. Update circle contribution amount and deadline for next cycle
        if next_cycle_amount.is_some() {
            circle.contribution_amount = contribution_amount;
            // Clear the next cycle amount since it has been applied
            env.storage().instance().remove(&DataKey::NextCycleAmount(circle_id));
        }
        circle.deadline_timestamp = current_time + circle.cycle_duration;
        env.storage().instance().set(&DataKey::Circle(circle_id), &circle);

        // 11. Mark as Paid in the old format for backward compatibility
        env.storage().instance().set(&DataKey::Deposit(circle_id, user.clone()), &true);

        // 12. Emit contribution event (MASKED if privacy is enabled)
        if privacy_masked {
            // Emit masked event - only member ID and success flag, NO amount
            let event = ContributionMaskedEvent {
                member_id: user,
                success: true,
            };
            env.events().publish((Symbol::new(&env, "contribution_masked"),), event);
        } else {
            // Emit regular contribution event with amount (for non-privacy circles)
            env.events().publish((Symbol::new(&env, "contribution"),), (user, contribution_amount));
        }
    }

    fn transfer_admin(env: Env, current_admin: Address, new_admin: Address) {
        // 1. Authorization: The current admin must sign this transaction
        current_admin.require_auth();

        // 2. Get the current admin from storage
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .unwrap_or_else(|| panic!("No admin set"));

        // 3. Verify the caller is the current admin
        if stored_admin != current_admin {
            panic!("Caller is not the current admin");
        }

        // 4. Update the admin in storage
        env.storage().instance().set(&DataKey::Admin, &new_admin);

        // 5. Emit the AdminChanged event
        let event = AdminChangedEvent {
            old_admin: current_admin,
            new_admin: new_admin,
        };
        env.events().publish((Symbol::new(&env, "admin_changed"),), event);
    }

    fn set_next_cycle_amount(env: Env, admin: Address, circle_id: u64, amount: i128) {
        // 1. Authorization: The admin must sign this transaction
        admin.require_auth();

        // 2. Verify the caller is the admin
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .unwrap_or_else(|| panic!("No admin set"));
        
        if stored_admin != admin {
            panic!("Caller is not the admin");
        }

        // 3. Verify the circle exists
        let _circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id))
            .unwrap_or_else(|| panic!("Circle does not exist"));

        // 4. Set the next cycle amount
        env.storage().instance().set(&DataKey::NextCycleAmount(circle_id), &amount);
    }

    fn configure_co_winners(env: Env, admin: Address, circle_id: u64, enabled: bool, max_winners: u32, split_method: u32) {
        // 1. Authorization: The admin must sign this transaction
        admin.require_auth();

        // 2. Verify the caller is the admin
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .unwrap_or_else(|| panic!("No admin set"));
        
        if stored_admin != admin {
            panic!("Caller is not the admin");
        }

        // 3. Verify the circle exists
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id))
            .unwrap_or_else(|| panic!("Circle does not exist"));

        // 4. Validate max_winners doesn't exceed member count
        if max_winners > circle.member_count {
            panic!("Max winners cannot exceed member count");
        }

        // 5. Create and store co-winners configuration
        let co_winners_config = CoWinnersConfig {
            enabled,
            max_winners,
            split_method,
        };
        env.storage().instance().set(&DataKey::CoWinnersConfig(circle_id), &co_winners_config);
    }

    fn distribute_funds(env: Env, admin: Address, circle_id: u64, co_winners: Vec<Address>) {
        // 1. Authorization: The admin must sign this transaction
        admin.require_auth();

        // 2. Verify the caller is the admin
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .unwrap_or_else(|| panic!("No admin set"));
        
        if stored_admin != admin {
            panic!("Caller is not the admin");
        }

        // 3. Get the circle info
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id))
            .unwrap_or_else(|| panic!("Circle does not exist"));

        // 4. Get co-winners configuration
        let co_winners_config: CoWinnersConfig = env.storage().instance().get(&DataKey::CoWinnersConfig(circle_id))
            .unwrap_or_else(|| CoWinnersConfig {
                enabled: false,
                max_winners: 1,
                split_method: 0,
            });

        // 5. Calculate total pool amount (total contributions minus fees)
        let total_contributions = circle.contribution_amount * circle.member_count as i128;
        
        // Calculate fees (1% of total contributions)
        let total_fees = total_contributions / 100; // 1% fee
        
        // Update Group Reserve with fees
        let mut reserve_balance: u64 = env.storage().instance().get(&DataKey::GroupReserve).unwrap_or(0);
        reserve_balance += total_fees as u64;
        env.storage().instance().set(&DataKey::GroupReserve, &reserve_balance);
        
        // Net amount to distribute (total contributions minus fees)
        let net_pool = total_contributions - total_fees;

        // 6. Handle co-winners logic
        if co_winners_config.enabled && !co_winners.is_empty() {
            // Validate co-winners count
            if co_winners.len() as u32 > co_winners_config.max_winners {
                panic!("Too many co-winners specified");
            }

            // Calculate shares based on split method
            let mut dust_amount = 0i128;
            let shares: Vec<i128> = if co_winners_config.split_method == 0 {
                // Equal split
                let base_share = net_pool / co_winners.len() as i128;
                dust_amount = net_pool - (base_share * co_winners.len() as i128);
                co_winners.iter().map(|_| base_share).collect::<Vec<i128>>()
            } else {
                // Proportional split based on contributions
                let mut total_private_contributions = 0i128;
                let mut contributions = Vec::new();
                
                for winner in &co_winners {
                    let key = DataKey::PrivateContribution(circle_id, winner.clone());
                    let contrib: i128 = env.storage().instance()
                        .get(&key)
                        .unwrap_or_else(|| circle.contribution_amount);
                    contributions.push(contrib);
                    total_private_contributions += contrib;
                }
                
                let mut shares = Vec::new();
                for contrib in contributions {
                    let share = (contrib * net_pool) / total_private_contributions;
                    shares.push(share);
                }
                
                // Calculate dust
                let total_distributed: i128 = shares.iter().sum();
                dust_amount = net_pool - total_distributed;
                shares
            };

            // Add dust to first co-winner (maintaining 100% accounting precision)
            if dust_amount > 0 {
                let mut updated_shares: Vec<i128> = shares;
                updated_shares[0] += dust_amount;
                
                // Set claimable balances for co-winners
                for (i, winner) in co_winners.iter().enumerate() {
                    let share_amount: i128 = updated_shares[i];
                    let key = DataKey::ClaimableBalance(circle_id, winner.clone());
                    env.storage().instance().set(&key, &share_amount);
                }
            } else {
                // Set claimable balances for co-winners
                for (i, winner) in co_winners.iter().enumerate() {
                    let share_amount: i128 = shares[i];
                    let key = DataKey::ClaimableBalance(circle_id, winner.clone());
                    env.storage().instance().set(&key, &share_amount);
                }
            }

            // Store current winners for record
            env.storage().instance().set(&DataKey::CurrentWinners(circle_id), &co_winners);
            
            // Emit co-winners distribution event
            env.events().publish((Symbol::new(&env, "co_winners_distributed"),), (circle_id, co_winners.len(), net_pool));
        } else {
            // Single winner logic (backwards compatibility)
            let share_per_member = net_pool / circle.member_count as i128;
            env.storage().instance().set(&DataKey::ClaimableBalance(circle_id, admin), &share_per_member);
        }
    }

    fn claim(env: Env, user: Address, circle_id: u64) {
        // 1. Authorization: The user must sign this transaction
        user.require_auth();

        // 2. Get the claimable balance for this user
        let claimable_balance: i128 = env.storage().instance().get(&DataKey::ClaimableBalance(circle_id, user.clone()))
            .unwrap_or_else(|| panic!("No claimable balance for this user"));

        // 3. Get the circle info to get the token address
        let circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id))
            .unwrap_or_else(|| panic!("Circle does not exist"));

        // 4. Create the token client
        let client = token::Client::new(&env, &circle.token);

        // 5. Transfer the funds to the user
        client.transfer(
            &env.current_contract_address(),
            &user,
            &claimable_balance,
        );

        // 6. Clear the claimable balance
        env.storage().instance().set(&DataKey::ClaimableBalance(circle_id, user), &0i128);
    }

    fn create_proposal(env: Env, proposer: Address, circle_id: u64, proposal_type: u32, description: String, voting_deadline: u64) -> u64 {
        // 1. Authorization: The proposer must sign this transaction
        proposer.require_auth();

        // 2. Verify the circle exists and user is a member
        let _circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(circle_id))
            .unwrap_or_else(|| panic!("Circle does not exist"));
        
        let member_key = DataKey::Member(proposer.clone());
        let member: Member = env.storage().instance().get(&member_key)
            .unwrap_or_else(|| panic!("User is not a member of this circle"));

        // 3. Get proposal ID (increment counter)
        let mut proposal_count: u64 = env.storage().instance().get(&DataKey::CircleCount).unwrap_or(0);
        proposal_count += 1;

        // 4. Calculate proposer's composite voting power
        let proposer_reputation: u64 = env.storage().instance().get(&DataKey::UserReputation(proposer.clone())).unwrap_or(0);
        
        // Get current cycle contributions for this member
        let private_key = DataKey::PrivateContribution(circle_id, proposer.clone());
        let current_cycle_contrib: i128 = env.storage().instance().get(&private_key).unwrap_or(0);
        
        // Composite voting power = (reputation score * 10) + (current contributions / 1000) + base power
        let reputation_power = proposer_reputation * 10; // Reputation has higher weight
        let contribution_power = (current_cycle_contrib / 1000) as u64; // Scale down contributions
        let base_power = 100; // Base voting power for all members
        
        let voting_power = reputation_power + contribution_power + base_power;

        // 5. Create the proposal
        let current_time = env.ledger().timestamp();
        let proposal = VotingProposal {
            id: proposal_count,
            circle_id,
            proposal_type,
            description,
            proposer: proposer.clone(),
            created_at: current_time,
            voting_deadline,
            yes_votes: 0,
            no_votes: 0,
            total_voting_power: voting_power,
            is_executed: false,
        };

        // 6. Store the proposal
        env.storage().instance().set(&DataKey::VotingProposal(proposal_count), &proposal);

        // 7. Emit proposal creation event
        env.events().publish((Symbol::new(&env, "proposal_created"),), (proposal_count, circle_id, proposer, proposal_type));

        // 8. Return proposal ID
        proposal_count
    }

    fn vote(env: Env, voter: Address, proposal_id: u64, vote: bool) {
        // 1. Authorization: The voter must sign this transaction
        voter.require_auth();

        // 2. Get the proposal
        let mut proposal: VotingProposal = env.storage().instance().get(&DataKey::VotingProposal(proposal_id))
            .unwrap_or_else(|| panic!("Proposal does not exist"));

        // 3. Check if voting is still open
        let current_time = env.ledger().timestamp();
        if current_time > proposal.voting_deadline {
            panic!("Voting period has ended");
        }

        // 4. Check if user has already voted
        let vote_key = DataKey::Vote(proposal_id, voter.clone());
        if env.storage().instance().has(&vote_key) {
            panic!("User has already voted on this proposal");
        }

        // 5. Calculate voter's composite voting power
        let voter_reputation: u64 = env.storage().instance().get(&DataKey::UserReputation(voter.clone())).unwrap_or(0);
        
        // Get current cycle contributions for this voter
        let private_key = DataKey::PrivateContribution(proposal.circle_id, voter.clone());
        let current_cycle_contrib: i128 = env.storage().instance().get(&private_key).unwrap_or(0);
        
        // Composite voting power = (reputation score * 10) + (current contributions / 1000) + base power
        let reputation_power = voter_reputation * 10; // Reputation has higher weight
        let contribution_power = (current_cycle_contrib / 1000) as u64; // Scale down contributions
        let base_power = 100; // Base voting power for all members
        
        let voting_power = reputation_power + contribution_power + base_power;

        // 6. Record the vote
        let vote_record = VoteCast {
            proposal_id,
            voter: voter.clone(),
            vote,
            voting_power,
        };
        env.storage().instance().set(&vote_key, &vote_record);

        // 7. Update proposal vote counts
        if vote {
            proposal.yes_votes += voting_power;
        } else {
            proposal.no_votes += voting_power;
        }
        proposal.total_voting_power += voting_power;

        // 8. Save updated proposal
        env.storage().instance().set(&DataKey::VotingProposal(proposal_id), &proposal);

        // 9. Emit vote event
        env.events().publish((Symbol::new(&env, "vote_cast"),), (proposal_id, voter, vote, voting_power));
    }

    fn execute_proposal(env: Env, executor: Address, proposal_id: u64) {
        // 1. Authorization: The executor must sign this transaction
        executor.require_auth();

        // 2. Get the proposal
        let mut proposal: VotingProposal = env.storage().instance().get(&DataKey::VotingProposal(proposal_id))
            .unwrap_or_else(|| panic!("Proposal does not exist"));

        // 3. Check if proposal has already been executed
        if proposal.is_executed {
            panic!("Proposal has already been executed");
        }

        // 4. Check if voting period has ended
        let current_time = env.ledger().timestamp();
        if current_time <= proposal.voting_deadline {
            panic!("Voting period has not ended yet");
        }

        // 5. Check if proposal passed (simple majority)
        if proposal.yes_votes <= proposal.no_votes {
            panic!("Proposal did not pass");
        }

        // 6. Execute proposal based on type
        match proposal.proposal_type {
            0 => {
                // Meeting date change - update circle deadline
                let mut circle: CircleInfo = env.storage().instance().get(&DataKey::Circle(proposal.circle_id))
                    .unwrap_or_else(|| panic!("Circle does not exist"));
                
                // Parse new deadline from description (simplified - in real implementation, 
                // description would contain structured data)
                let new_deadline = proposal.created_at + 7 * 24 * 3600; // Example: 7 days from proposal creation
                circle.deadline_timestamp = new_deadline;
                
                env.storage().instance().set(&DataKey::Circle(proposal.circle_id), &circle);
            },
            1 => {
                // New member admission - extract member address from description
                // In real implementation, description would contain the new member address
                // For now, this is a placeholder that would be implemented with proper parsing
            },
            _ => {
                // Other proposal types - custom logic would go here
            }
        }

        // 7. Mark proposal as executed
        proposal.is_executed = true;
        env.storage().instance().set(&DataKey::VotingProposal(proposal_id), &proposal);
    }

    fn update_reputation(env: Env, admin: Address, user: Address, reputation_score: u64) {
        // 1. Authorization: The admin must sign this transaction
        admin.require_auth();

        // 2. Verify the caller is the admin
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .unwrap_or_else(|| panic!("No admin set"));
        
        if stored_admin != admin {
            panic!("Caller is not the admin");
        }

        // 3. Get current reputation for comparison
        let current_reputation: u64 = env.storage().instance().get(&DataKey::UserReputation(user.clone())).unwrap_or(0);
        
        // 4. Update user reputation
        env.storage().instance().set(&DataKey::UserReputation(user), &reputation_score);

        // 5. Emit reputation update event
        env.events().publish((Symbol::new(&env, "reputation_updated"),), (user, current_reputation, reputation_score, admin));
        
        // 6. If reputation was increased, check if user can now join higher-tier circles
        if reputation_score > current_reputation {
            env.events().publish((Symbol::new(&env, "reputation_upgraded"),), (user, current_reputation, reputation_score));
        }
    }

    fn get_private_contribution(env: Env, user: Address, circle_id: u64, target_member: Address) -> i128 {
        // 1. Authorization: The user must sign this transaction
        user.require_auth();

        // 2. Verify the user is a member of the circle
        let member_key = DataKey::Member(user.clone());
        let _member: Member = env.storage().instance().get(&member_key)
            .unwrap_or_else(|| panic!("User is not a member of this circle"));

        // 3. Get the private contribution amount
        let contribution: i128 = env.storage().instance()
            .get(&DataKey::PrivateContribution(circle_id, target_member))
            .unwrap_or_else(|| panic!("Private contribution not found for target member"));

        // 4. Return the contribution amount
        contribution
    }
}
