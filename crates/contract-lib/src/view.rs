use crate::{Contract, ContractExt};
use near_sdk::{json_types::U128, near_bindgen, AccountId};
use primitive_types::U256;

#[near_bindgen]
impl Contract {
    pub fn get_whitelisted_tokens(&self) -> Vec<AccountId> {
        self.token_whitelist.get().clone()
    }

    pub fn get_undistributed_rewards(&self) -> Vec<(AccountId, U128)> {
        self.rewards
            .iter()
            .map(|(token_id, amount)| (token_id.clone(), U128(*amount)))
            .collect()
    }

    pub fn get_deposits(&self) -> Vec<(AccountId, U128)> {
        self.deposits
            .iter()
            .map(|(token_id, amount)| (token_id.clone(), U128(*amount)))
            .collect()
    }

    pub fn simulate_burn(&self, shares: U128) -> Vec<(AccountId, U128)> {
        self.rewards
            .iter()
            .map(|(token_id, deposit)| {
                let amount = (U256::from(shares.0) * U256::from(*deposit)
                    / U256::from(self.shares))
                .as_u128();
                (token_id.clone(), U128(amount))
            })
            .collect()
    }
}
