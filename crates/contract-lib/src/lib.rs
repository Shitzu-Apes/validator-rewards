mod owner;
mod view;

use std::cmp;

use near_contract_standards::{
    fungible_token::{
        core::ext_ft_core,
        events::{FtBurn, FtTransfer},
        metadata::{FungibleTokenMetadata, FungibleTokenMetadataProvider},
        receiver::{ext_ft_receiver, FungibleTokenReceiver},
        FungibleTokenCore, FungibleTokenResolver,
    },
    storage_management::{StorageBalance, StorageBalanceBounds, StorageManagement},
};
#[allow(deprecated)]
use near_sdk::{
    assert_one_yocto,
    borsh::{BorshDeserialize, BorshSerialize},
    env,
    json_types::U128,
    near_bindgen, require,
    store::{Lazy, TreeMap, UnorderedMap},
    AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, PromiseOrValue,
};
use near_sdk::{serde_json, PromiseResult};
use primitive_types::U256;

const GAS_FOR_BURN: Gas = Gas::from_tgas(5);
const GAS_FOR_FT_TRANSFER: Gas = Gas::from_tgas(10);
const GAS_FOR_FT_TRANSFER_CALL: Gas = Gas::from_tgas(60);
const GAS_FOR_RESOLVE_TRANSFER: Gas = Gas::from_tgas(5);

#[derive(BorshStorageKey, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
pub enum StorageKey {
    Accounts,
    Deposits,
    Rewards,
    TokenWhitelist,
}

#[near_bindgen]
#[derive(BorshSerialize, BorshDeserialize, PanicOnDefault)]
#[borsh(crate = "near_sdk::borsh")]
#[allow(deprecated)]
pub struct Contract {
    owner: AccountId,
    validator: AccountId,
    accounts: TreeMap<AccountId, u128>,
    deposits: UnorderedMap<AccountId, u128>,
    rewards: UnorderedMap<AccountId, u128>,
    shares: u128,
    token_whitelist: Lazy<Vec<AccountId>>,
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(owner: AccountId, validator: AccountId, token_whitelist: Vec<AccountId>) -> Self {
        #[allow(deprecated)]
        Self {
            owner,
            validator,
            accounts: TreeMap::new(StorageKey::Accounts),
            deposits: UnorderedMap::new(StorageKey::Deposits),
            rewards: UnorderedMap::new(StorageKey::Rewards),
            shares: 0,
            token_whitelist: Lazy::new(StorageKey::TokenWhitelist, token_whitelist),
        }
    }

    #[payable]
    pub fn burn(&mut self) -> U128 {
        assert_one_yocto();
        require!(
            env::prepaid_gas()
                >= GAS_FOR_BURN
                    .checked_add(
                        GAS_FOR_FT_TRANSFER
                            .checked_mul(self.rewards.len() as u64)
                            .unwrap()
                    )
                    .unwrap(),
            "Not enough gas attached"
        );
        let sender_id = env::predecessor_account_id();

        let balance = self.accounts.remove(&sender_id).unwrap();

        for (token_id, deposit) in self.rewards.iter_mut() {
            let amount =
                (U256::from(balance) * U256::from(*deposit) / U256::from(self.shares)).as_u128();
            *deposit -= amount;
            ext_ft_core::ext(token_id.clone())
                .with_unused_gas_weight(1)
                .with_attached_deposit(NearToken::from_yoctonear(1))
                .ft_transfer(sender_id.clone(), amount.into(), None);
        }
        self.shares -= balance;

        FtBurn {
            owner_id: &sender_id,
            amount: balance.into(),
            memo: None,
        }
        .emit();

        U128(balance)
    }
}

#[allow(unused_variables)]
#[near_bindgen]
impl FungibleTokenCore for Contract {
    #[payable]
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        assert_one_yocto();
        let sender_id = env::predecessor_account_id();
        let amount = amount.0;
        require!(
            sender_id == self.validator,
            "Only validator can distribute tokens"
        );
        require!(amount > 0, "The amount should be a positive number");

        if !self.accounts.contains_key(&receiver_id) {
            self.accounts.insert(receiver_id.clone(), 0);
        }
        let balance = self.accounts.get_mut(&receiver_id).unwrap();
        *balance += amount;

        let balance = self.accounts.get_mut(&sender_id).unwrap();
        *balance -= amount;

        FtTransfer {
            old_owner_id: &self.validator,
            new_owner_id: &receiver_id,
            amount: U128(amount),
            memo: memo.as_deref(),
        }
        .emit();
    }

    #[payable]
    fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<U128> {
        assert_one_yocto();
        require!(
            env::prepaid_gas() > GAS_FOR_FT_TRANSFER_CALL,
            "More gas is required"
        );
        let sender_id = env::predecessor_account_id();
        let amount = amount.0;
        require!(sender_id == self.owner, "Only owner can call this function");
        require!(amount > 0, "The amount should be a positive number");

        if !self.accounts.contains_key(&receiver_id) {
            self.accounts.insert(receiver_id.clone(), 0);
        }
        let balance = self.accounts.get_mut(&receiver_id).unwrap();
        *balance += amount;

        let balance = self.accounts.get_mut(&sender_id).unwrap();
        *balance -= amount;

        let receiver_gas = env::prepaid_gas()
            .checked_sub(GAS_FOR_FT_TRANSFER_CALL)
            .unwrap_or_else(|| env::panic_str("Prepaid gas overflow"));
        ext_ft_receiver::ext(receiver_id.clone())
            .with_unused_gas_weight(1)
            .ft_on_transfer(sender_id.clone(), amount.into(), msg)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_FOR_RESOLVE_TRANSFER)
                    .ft_resolve_transfer(sender_id, receiver_id, amount.into()),
            )
            .into()
    }

    fn ft_total_supply(&self) -> U128 {
        self.shares.into()
    }

    fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        self.accounts
            .get(&account_id)
            .copied()
            .unwrap_or_default()
            .into()
    }
}

#[near_bindgen]
impl FungibleTokenReceiver for Contract {
    #[allow(unused_variables)]
    fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        require!(sender_id == self.owner, "Only owner can deposit");
        let token_id = env::predecessor_account_id();
        require!(
            self.token_whitelist.contains(&token_id),
            "Token not whitelisted"
        );

        match self.deposits.get_mut(&token_id) {
            Some(deposit) => {
                *deposit += amount.0;
            }
            None => {
                self.deposits.insert(token_id, amount.0);
            }
        }
        PromiseOrValue::Value(0.into())
    }
}

#[near_bindgen]
impl FungibleTokenResolver for Contract {
    fn ft_resolve_transfer(
        &mut self,
        sender_id: AccountId,
        receiver_id: AccountId,
        amount: U128,
    ) -> U128 {
        let amount = amount.0;

        // Get the unused amount from the `ft_on_transfer` call result.
        let unused_amount = match env::promise_result(0) {
            PromiseResult::Successful(value) => {
                if let Ok(unused_amount) = serde_json::from_slice::<U128>(&value) {
                    cmp::min(amount, unused_amount.0)
                } else {
                    amount
                }
            }
            PromiseResult::Failed => amount,
        };

        if unused_amount > 0 {
            let receiver_balance = self.accounts.get_mut(&receiver_id).unwrap();
            let refund_amount = std::cmp::min(*receiver_balance, unused_amount);
            *receiver_balance -= refund_amount;

            let sender_balance = self.accounts.get_mut(&sender_id).unwrap();
            *sender_balance += refund_amount;

            FtTransfer {
                old_owner_id: &receiver_id,
                new_owner_id: &sender_id,
                amount: U128(refund_amount),
                memo: Some("refund"),
            }
            .emit();
            let used_amount = amount - refund_amount;
            U128(used_amount)
        } else {
            U128(amount)
        }
    }
}

#[allow(unused_variables)]
#[near_bindgen]
impl StorageManagement for Contract {
    #[payable]
    fn storage_deposit(
        &mut self,
        account_id: Option<AccountId>,
        registration_only: Option<bool>,
    ) -> StorageBalance {
        env::panic_str("unimplemented");
    }

    #[payable]
    fn storage_withdraw(&mut self, amount: Option<NearToken>) -> StorageBalance {
        env::panic_str("unimplemented");
    }

    #[payable]
    fn storage_unregister(&mut self, force: Option<bool>) -> bool {
        env::panic_str("unimplemented");
    }

    fn storage_balance_bounds(&self) -> StorageBalanceBounds {
        StorageBalanceBounds {
            min: NearToken::from_yoctonear(0),
            max: None,
        }
    }

    fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        Some(StorageBalance {
            total: NearToken::from_millinear(50),
            available: NearToken::from_yoctonear(0),
        })
    }
}

#[near_bindgen]
impl FungibleTokenMetadataProvider for Contract {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        FungibleTokenMetadata {
            spec: "ft-1.0.0".to_string(),
            name: "Shitzu Validator Reward".to_string(),
            symbol: "DOGSHIT".to_string(),
            icon: None,
            reference: None,
            reference_hash: None,
            decimals: 24,
        }
    }
}
