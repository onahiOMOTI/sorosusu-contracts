#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Symbol, Vec};

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
    // New: Early payout requests
    EarlyPayoutRequest(u64, Address),
}

#[contracttype]
#[derive(Clone)]
pub struct CircleInfo {
    pub id: u64,
    pub creator: Address,
    pub contribution_amount: i128, // Amount in Stroops (e.g. 50 USDC)
    pub max_members: u32,
    pub members: Vec<Address>,
    pub current_recipient: Address, // Who gets the pot this week?
    pub is_active: bool,
    pub token: Address, // The token used (USDC, XLM)
}

// --- CONTRACT TRAIT ---

pub trait SoroSusuTrait {
    // Initialize the contract
    fn init(env: Env, admin: Address);

    // Create a new savings circle
    fn create_circle(
        env: Env,
        creator: Address,
        amount: i128,
        max_members: u32,
        token: Address,
    ) -> u64;

    // Join an existing circle
    fn join_circle(env: Env, user: Address, circle_id: u64);

    // Make a deposit (Pay your weekly/monthly due)
    fn deposit(env: Env, user: Address, circle_id: u64);

    // Request early payout (emergency)
    fn request_early_payout(env: Env, user: Address, circle_id: u64);

    // Approve early payout (admin only)
    fn approve_early_payout(env: Env, admin: Address, circle_id: u64, user: Address);
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

    fn create_circle(
        env: Env,
        creator: Address,
        amount: i128,
        max_members: u32,
        token: Address,
    ) -> u64 {
        // 1. Get the current Circle Count
        let mut circle_count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::CircleCount)
            .unwrap_or(0);

        // 2. Increment the ID for the new circle
        circle_count += 1;

        // 3. Create the Circle Data Struct
        let new_circle = CircleInfo {
            id: circle_count,
            creator: creator.clone(),
            contribution_amount: amount,
            max_members,
            members: Vec::new(&env),    // Start with empty list
            current_recipient: creator, // Temporary placeholder
            is_active: true,
            token,
        };

        // 4. Save the Circle and the new Count
        env.storage()
            .instance()
            .set(&DataKey::Circle(circle_count), &new_circle);
        env.storage()
            .instance()
            .set(&DataKey::CircleCount, &circle_count);

        // 5. Return the new ID
        circle_count
    }

    fn join_circle(env: Env, user: Address, circle_id: u64) {
        // 1. Authorization: The user MUST sign this transaction
        user.require_auth();

        // 2. Retrieve the circle data
        // We use 'unwrap()' here effectively saying "If this ID doesn't exist, fail immediately"
        let mut circle: CircleInfo = env
            .storage()
            .instance()
            .get(&DataKey::Circle(circle_id))
            .unwrap();

        // 3. Check if the circle is full
        if circle.members.len() >= circle.max_members {
            panic!("Circle is full");
        }

        // 4. Check if user is already a member to prevent duplicates
        if circle.members.contains(&user) {
            panic!("User is already a member");
        }

        // 5. Add the user to the list
        circle.members.push_back(user.clone());

        // 6. Save the updated circle back to storage
        env.storage()
            .instance()
            .set(&DataKey::Circle(circle_id), &circle);
    }

    fn deposit(env: Env, user: Address, circle_id: u64) {
        // 1. Authorization: The user must sign this!
        user.require_auth();

        // 2. Load the Circle Data
        let circle: CircleInfo = env
            .storage()
            .instance()
            .get(&DataKey::Circle(circle_id))
            .unwrap();

        // 3. Check if user is actually a member
        if !circle.members.contains(&user) {
            panic!("User is not a member of this circle");
        }

        // 4. Create the Token Client (The "Remote Control")
        // This tells Soroban: "We want to talk to the token contract at this address"
        let client = token::Client::new(&env, &circle.token);

        // 5. Transfer the Money
        // From: User
        // To: This Contract (env.current_contract_address())
        // Amount: The circle's contribution amount
        client.transfer(
            &user,
            &env.current_contract_address(),
            &circle.contribution_amount,
        );

        // 6. Mark as Paid
        // We save "True" for this specific (CircleID, User) combination
        env.storage()
            .instance()
            .set(&DataKey::Deposit(circle_id, user), &true);
    }

    fn request_early_payout(env: Env, user: Address, circle_id: u64) {
        // 1. Authorization: The user must sign this transaction
        user.require_auth();

        // 2. Load the Circle Data
        let circle: CircleInfo = env
            .storage()
            .instance()
            .get(&DataKey::Circle(circle_id))
            .unwrap();

        // 3. Check if user is a member of the circle
        if !circle.members.contains(&user) {
            panic!("User is not a member of this circle");
        }

        // 4. Check if user already has a pending request
        if env
            .storage()
            .instance()
            .has(&DataKey::EarlyPayoutRequest(circle_id, user.clone()))
        {
            panic!("User already has a pending early payout request");
        }

        // 5. Store the early payout request
        env.storage()
            .instance()
            .set(&DataKey::EarlyPayoutRequest(circle_id, user), &true);
    }

    fn approve_early_payout(env: Env, admin: Address, circle_id: u64, user: Address) {
        // 1. Authorization: The admin must sign this transaction
        admin.require_auth();

        // 2. Verify the caller is actually the admin
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != stored_admin {
            panic!("Not authorized: Only admin can approve early payouts");
        }

        // 3. Load the Circle Data
        let mut circle: CircleInfo = env
            .storage()
            .instance()
            .get(&DataKey::Circle(circle_id))
            .unwrap();

        // 4. Check if user has a pending early payout request
        if !env
            .storage()
            .instance()
            .has(&DataKey::EarlyPayoutRequest(circle_id, user.clone()))
        {
            panic!("No pending early payout request found for this user");
        }

        // 5. Check if user is the current recipient (no swap needed)
        if circle.current_recipient == user {
            panic!("User is already the current recipient");
        }

        // 6. Find the user's position in the members vector
        let user_index = circle
            .members
            .iter()
            .position(|member| member == &user)
            .unwrap();

        // 7. Find current recipient's position
        let current_recipient_index = circle
            .members
            .iter()
            .position(|member| member == &circle.current_recipient)
            .unwrap();

        // 8. Swap positions in the queue
        let mut members = circle.members;
        members.swap(user_index, current_recipient_index);

        // 9. Update the circle with new member order and current recipient
        circle.members = members;
        circle.current_recipient = user.clone();

        // 10. Save the updated circle
        env.storage()
            .instance()
            .set(&DataKey::Circle(circle_id), &circle);

        // 11. Remove the early payout request (it's been processed)
        env.storage()
            .instance()
            .remove(&DataKey::EarlyPayoutRequest(circle_id, user));

        // 12. Transfer the available funds to the user
        let client = token::Client::new(&env, &circle.token);

        // Calculate available balance (all deposits made so far)
        let mut total_deposits = 0i128;
        for member in circle.members.iter() {
            if env
                .storage()
                .instance()
                .get(&DataKey::Deposit(circle_id, member))
                .unwrap_or(false)
            {
                total_deposits += circle.contribution_amount;
            }
        }

        // Transfer the available funds to the new recipient
        if total_deposits > 0 {
            client.transfer(&env.current_contract_address(), &user, &total_deposits);
        }
    }
}
