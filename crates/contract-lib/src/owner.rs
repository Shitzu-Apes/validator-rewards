use crate::{Contract, ContractExt};
use near_contract_standards::fungible_token::{core::ext_ft_core, events::FtMint};
use near_sdk::{env, json_types::U128, near_bindgen, require, AccountId, NearToken, Promise};

#[near_bindgen]
impl Contract {
    pub fn whitelist_add_token(&mut self, token_id: AccountId) {
        self.require_owner();
        self.token_whitelist.push(token_id);
    }

    pub fn whitelist_remove_token(&mut self, token_id: AccountId) {
        self.require_owner();
        if let Some(index) = self
            .token_whitelist
            .iter()
            .enumerate()
            .find_map(|(index, token)| {
                if token == &token_id {
                    Some(index)
                } else {
                    None
                }
            })
        {
            self.token_whitelist.remove(index);
        } else {
            env::panic_str("Token not found in whitelist")
        }
    }

    pub fn withdraw(&mut self, token_id: AccountId, amount: U128) -> Promise {
        self.require_owner();
        let deposit = self.deposits.get_mut(&token_id).unwrap();
        *deposit -= amount.0;
        ext_ft_core::ext(token_id)
            .with_unused_gas_weight(1)
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .ft_transfer(self.owner.clone(), amount, None)
    }

    pub fn mint(&mut self, shares: U128) {
        self.require_owner();
        self.shares += shares.0;
        self.accounts.insert(self.owner.clone(), self.shares);
        for (token_id, amount) in self.deposits.drain() {
            if let Some(reward) = self.rewards.get_mut(&token_id) {
                *reward += amount;
            } else {
                self.rewards.insert(token_id, amount);
            }
        }
        FtMint {
            owner_id: &self.owner,
            amount: self.shares.into(),
            memo: None,
        }
        .emit();
    }

    #[private]
    pub fn migrate(&mut self) {
        // add migration here if needed
    }

    pub fn upgrade(&self) -> Promise {
        self.require_owner();

        let code = env::input().expect("Error: No input").to_vec();

        Promise::new(env::current_account_id())
            .deploy_contract(code)
            .as_return()
    }

    pub fn upgrade_and_migrate(&self) -> Promise {
        self.require_owner();

        let code = env::input().expect("Error: No input").to_vec();

        Promise::new(env::current_account_id())
            .deploy_contract(code)
            .then(Self::ext(env::current_account_id()).migrate())
            .as_return()
    }
}

impl Contract {
    fn require_owner(&self) {
        require!(
            env::predecessor_account_id() == self.owner,
            "Only owner can call this function"
        );
    }
}
