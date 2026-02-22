#![cfg_attr(test, allow(dead_code))]
#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contractmeta, symbol_short, token, Address, Env, Map,
    Symbol, Vec,
};

const FEE_BASIS_POINTS_KEY: Symbol = symbol_short!("fee_bps");
const TREASURY_KEY: Symbol = symbol_short!("treasury");
const ADMIN_KEY: Symbol = symbol_short!("admin");
const MEMBERS_KEY: Symbol = symbol_short!("members");
const CONTRIBS_KEY: Symbol = symbol_short!("contribs");
const MAX_BASIS_POINTS: u32 = 10_000;

contractmeta!(
    key = "Description",
    val = "SoroSusu ROSCA protocol with protocol payout fee"
);

#[contract]
pub struct SorosusuContracts;

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    Unauthorized = 1005,
    InvalidFeeConfig = 1006,
    MemberNotFound = 1007,
    PenaltyExceedsContribution = 1008,
    MemberAlreadyExists = 1009,
}

#[contractimpl]
impl SorosusuContracts {
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
        Ok(())
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
        Self::apply_member_swap(&env, old_member, new_member)
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

    fn require_admin(env: &Env) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&ADMIN_KEY)
            .ok_or(Error::Unauthorized)?;
        admin.require_auth();
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
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    fn setup(env: &Env) -> (SorosusuContractsClient<'_>, Address) {
        let contract_id = env.register_contract(None, SorosusuContracts);
        let admin = Address::generate(env);
        let client = SorosusuContractsClient::new(env, &contract_id);
        (client, admin)
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
        let (client, admin) = setup(&env);
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
        let (client, admin) = setup(&env);
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
    fn kick_member_fails_if_not_found() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        client.initialize(&admin);

        let dummy_token = Address::generate(&env);
        let dummy_member = Address::generate(&env);

        env.mock_all_auths();
        let result = client.try_kick_member(&dummy_token, &dummy_member, &0);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().unwrap(), Error::MemberNotFound);
    }

    #[test]
    fn swap_member_replaces_queue_spot_and_transfers_credit() {
        let env = Env::default();
        let (client, admin) = setup(&env);
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
        assert_eq!(stored_contribs.get(new_member).unwrap(), 750_i128);
        assert_eq!(stored_contribs.get(old_member), None);
    }

    #[test]
    fn swap_member_fails_if_old_member_missing() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        client.initialize(&admin);

        let old_member = Address::generate(&env);
        let new_member = Address::generate(&env);

        let members = Vec::new(&env);
        let contribs = Map::new(&env);
        seed_members_and_contribs(&env, &client.address, members, contribs);

        env.mock_all_auths();
        let result = client.try_swap_member(&old_member, &new_member);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().unwrap(), Error::MemberNotFound);
    }

    #[test]
    fn swap_member_by_admin_replaces_queue_spot_and_transfers_credit() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        client.initialize(&admin);

        let old_member = Address::generate(&env);
        let new_member = Address::generate(&env);
        let other_member = Address::generate(&env);

        let mut members = Vec::new(&env);
        members.push_back(old_member.clone());
        members.push_back(other_member.clone());

        let mut contribs = Map::new(&env);
        contribs.set(old_member.clone(), 900_i128);
        contribs.set(other_member.clone(), 300_i128);
        seed_members_and_contribs(&env, &client.address, members, contribs);

        env.mock_all_auths();
        client.swap_member_by_admin(&old_member, &new_member);

        let (stored_members, stored_contribs) = read_members_and_contribs(&env, &client.address);
        assert_eq!(stored_members.get(0).unwrap(), new_member.clone());
        assert_eq!(stored_members.get(1).unwrap(), other_member.clone());
        assert_eq!(stored_contribs.get(new_member).unwrap(), 900_i128);
        assert_eq!(stored_contribs.get(old_member), None);
    }
}
