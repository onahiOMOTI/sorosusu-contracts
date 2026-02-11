#![no_std]
use soroban_sdk::{contract, contracttype, contractimpl, Address, Env, Vec, Symbol};

// --- DATA STRUCTURES ---

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,           // The address that controls the protocol parameters
    Circle(u64),     // Map circle_id to CircleInfo
    Member(Address), // Map user address to their Reputation Score
    CircleCount,     // Counter for generating new circle IDs
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
    fn create_circle(env: Env, creator: Address, amount: i128, max_members: u32, token: Address) -> u64;

    // Join an existing circle
    fn join_circle(env: Env, user: Address, circle_id: u64);

    // Make a deposit (Pay your weekly/monthly due)
    fn deposit(env: Env, user: Address, circle_id: u64);
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

    fn create_circle(env: Env, creator: Address, amount: i128, max_members: u32, token: Address) -> u64 {
        // 1. Get the current Circle Count
        let mut circle_count: u64 = env.storage().instance().get(&DataKey::CircleCount).unwrap_or(0);
        
        // 2. Increment the ID for the new circle
        circle_count += 1;

        // 3. Create the Circle Data Struct
        let new_circle = CircleInfo {
            id: circle_count,
            creator: creator.clone(),
            contribution_amount: amount,
            max_members,
            members: Vec::new(&env), // Start with empty list
            current_recipient: creator, // Temporary placeholder
            is_active: true,
            token,
        };

        // 4. Save the Circle and the new Count
        env.storage().instance().set(&DataKey::Circle(circle_count), &new_circle);
        env.storage().instance().set(&DataKey::CircleCount, &circle_count);

        // 5. Return the new ID
        circle_count
    }

    fn join_circle(env: Env, user: Address, circle_id: u64) {
        // TODO: Logic to add user to members list
    }

    fn deposit(env: Env, user: Address, circle_id: u64) {
        // TODO: Logic to transfer tokens from user to contract
    }
}