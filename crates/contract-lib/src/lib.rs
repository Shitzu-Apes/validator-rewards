mod owner;
mod view;

use near_contract_standards::{
    fungible_token::{
        core::ext_ft_core,
        events::{FtBurn, FtTransfer},
        metadata::{FungibleTokenMetadata, FungibleTokenMetadataProvider},
        receiver::{ext_ft_receiver, FungibleTokenReceiver},
        FungibleTokenCore, FungibleTokenResolver,
    },
    non_fungible_token::TokenId,
    storage_management::{StorageBalance, StorageBalanceBounds, StorageManagement},
};
#[allow(deprecated)]
use near_sdk::store::UnorderedMap;
use near_sdk::{
    assert_one_yocto,
    borsh::{BorshDeserialize, BorshSerialize},
    env, ext_contract,
    json_types::U128,
    near_bindgen, require, serde_json,
    store::{Lazy, TreeMap},
    AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, PromiseOrValue, PromiseResult,
};
use primitive_types::U256;
use std::cmp;

const GAS_FOR_BURN: Gas = Gas::from_tgas(5);
const GAS_FOR_NFT_CHECK: Gas = Gas::from_tgas(5);
const GAS_FOR_FT_TRANSFER: Gas = Gas::from_tgas(10);
const GAS_FOR_FT_TRANSFER_CALL: Gas = Gas::from_tgas(60);
const GAS_FOR_RESOLVE_TRANSFER: Gas = Gas::from_tgas(5);

#[ext_contract(shitzu_nft)]
#[allow(dead_code)]
trait ShitzuNft {
    fn nft_supply_for_owner(&mut self, account_id: AccountId) -> u32;
}

#[ext_contract(rewarder)]
#[allow(dead_code)]
trait Rewarder {
    fn primary_nft_of(&mut self, account_id: AccountId) -> Option<(TokenId, U128)>;
    fn on_track_score(&mut self, primary_nft: TokenId, amount: U128);
}

#[derive(BorshStorageKey, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
pub enum StorageKey {
    Accounts,
    Deposits,
    Rewards,
    TokenWhitelist,
}

#[near_bindgen(contract_metadata(
    standard(standard = "148", version = "1.0.0")
))]
#[derive(BorshSerialize, BorshDeserialize, PanicOnDefault)]
#[borsh(crate = "near_sdk::borsh")]
#[allow(deprecated)]
pub struct Contract {
    owner: AccountId,
    validator: AccountId,
    rewarder: AccountId,
    shitzu_token: AccountId,
    shitzu_nft: AccountId,
    accounts: TreeMap<AccountId, u128>,
    deposits: UnorderedMap<AccountId, u128>,
    rewards: UnorderedMap<AccountId, u128>,
    shares: u128,
    token_whitelist: Lazy<Vec<AccountId>>,
}

#[derive(BorshSerialize, BorshDeserialize, PanicOnDefault)]
#[borsh(crate = "near_sdk::borsh")]
#[allow(deprecated)]
pub struct OldContract {
    owner: AccountId,
    validator: AccountId,
    shitzu_nft: AccountId,
    accounts: TreeMap<AccountId, u128>,
    deposits: UnorderedMap<AccountId, u128>,
    rewards: UnorderedMap<AccountId, u128>,
    shares: u128,
    token_whitelist: Lazy<Vec<AccountId>>,
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(
        owner: AccountId,
        validator: AccountId,
        shitzu_token: AccountId,
        shitzu_nft: AccountId,
        rewarder: AccountId,
        token_whitelist: Vec<AccountId>,
    ) -> Self {
        let mut accounts = TreeMap::new(StorageKey::Accounts);
        accounts.insert(owner.clone(), 0);
        #[allow(deprecated)]
        Self {
            owner,
            validator,
            rewarder,
            shitzu_token,
            shitzu_nft,
            accounts,
            deposits: UnorderedMap::new(StorageKey::Deposits),
            rewards: UnorderedMap::new(StorageKey::Rewards),
            shares: 0,
            token_whitelist: Lazy::new(StorageKey::TokenWhitelist, token_whitelist),
        }
    }

    #[payable]
    pub fn burn(&mut self) -> PromiseOrValue<U128> {
        assert_one_yocto();
        require!(
            env::prepaid_gas()
                >= GAS_FOR_BURN
                    .saturating_add(GAS_FOR_NFT_CHECK)
                    .saturating_add(
                        GAS_FOR_FT_TRANSFER
                            .checked_mul(self.rewards.len() as u64)
                            .unwrap()
                    ),
            "Not enough gas attached"
        );
        let sender_id = env::predecessor_account_id();

        require!(
            self.accounts.contains_key(&sender_id),
            "Account has no tokens"
        );

        if sender_id == self.owner {
            let balance = self.accounts.remove(&sender_id).unwrap();

            for (token_id, deposit) in self.rewards.iter_mut() {
                let amount = (U256::from(balance) * U256::from(*deposit) / U256::from(self.shares))
                    .as_u128();
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

            PromiseOrValue::Value(U128(balance))
        } else {
            PromiseOrValue::Promise(
                rewarder::ext(self.rewarder.clone())
                    .with_static_gas(GAS_FOR_NFT_CHECK)
                    .primary_nft_of(env::predecessor_account_id())
                    .then(
                        Self::ext(env::current_account_id())
                            .with_unused_gas_weight(1)
                            .on_burn(sender_id),
                    ),
            )
        }
    }

    #[private]
    pub fn on_burn(
        &mut self,
        sender_id: AccountId,
        #[callback_unwrap] primary_nft: Option<(TokenId, U128)>,
    ) -> U128 {
        let mut balance = self.accounts.remove(&sender_id).unwrap();

        if primary_nft.is_none() {
            let owner_refund = balance / 5;
            balance -= owner_refund;
            let owner_balance = self.accounts.get_mut(&self.owner).unwrap();
            *owner_balance += owner_refund;

            FtTransfer {
                old_owner_id: &sender_id,
                new_owner_id: &self.owner,
                amount: owner_refund.into(),
                memo: None,
            }
            .emit();
        }

        for (token_id, deposit) in self.rewards.iter_mut() {
            let amount =
                (U256::from(balance) * U256::from(*deposit) / U256::from(self.shares)).as_u128();
            *deposit -= amount;
            if token_id == &self.shitzu_token {
                if let Some((primary_nft, _)) = &primary_nft {
                    ext_ft_core::ext(token_id.clone())
                        .with_unused_gas_weight(1)
                        .with_attached_deposit(NearToken::from_yoctonear(1))
                        .ft_transfer(sender_id.clone(), amount.into(), None)
                        .then(
                            rewarder::ext(self.rewarder.clone())
                                .with_unused_gas_weight(1)
                                .on_track_score(primary_nft.clone(), (amount * 3).into()),
                        );
                } else {
                    ext_ft_core::ext(token_id.clone())
                        .with_unused_gas_weight(1)
                        .with_attached_deposit(NearToken::from_yoctonear(1))
                        .ft_transfer(sender_id.clone(), amount.into(), None);
                }
            } else {
                ext_ft_core::ext(token_id.clone())
                    .with_unused_gas_weight(1)
                    .with_attached_deposit(NearToken::from_yoctonear(1))
                    .ft_transfer(sender_id.clone(), amount.into(), None);
            }
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
            icon: Some("data:image/webp;base64,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".to_string()),
            reference: None,
            reference_hash: None,
            decimals: 24,
        }
    }
}
