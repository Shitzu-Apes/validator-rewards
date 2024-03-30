use near_contract_standards::{
    fungible_token::{
        core::ext_ft_core,
        events::{FtBurn, FtMint, FtTransfer},
        metadata::{FungibleTokenMetadata, FungibleTokenMetadataProvider},
        receiver::FungibleTokenReceiver,
        FungibleTokenCore,
    },
    storage_management::{StorageBalance, StorageBalanceBounds, StorageManagement},
};
use near_sdk::{
    assert_one_yocto,
    borsh::{BorshDeserialize, BorshSerialize},
    env,
    json_types::U128,
    near_bindgen, require,
    store::{Lazy, TreeMap},
    AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, PromiseOrValue,
};
use primitive_types::U256;

const GAS_FOR_BURN: Gas = Gas::from_tgas(5);
const GAS_FOR_FT_TRANSFER: Gas = Gas::from_tgas(10);

#[derive(BorshStorageKey, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
pub enum StorageKey {
    Accounts,
    Deposits,
    TokenWhitelist,
}

#[near_bindgen]
#[derive(BorshSerialize, BorshDeserialize, PanicOnDefault)]
#[borsh(crate = "near_sdk::borsh")]
pub struct Contract {
    owner: AccountId,
    validator: AccountId,
    accounts: TreeMap<AccountId, u128>,
    deposits: TreeMap<AccountId, u128>,
    shares: u128,
    is_minted: bool,
    token_whitelist: Lazy<Vec<AccountId>>,
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(owner: AccountId, validator: AccountId, token_whitelist: Vec<AccountId>) -> Self {
        Self {
            owner,
            validator,
            accounts: TreeMap::new(StorageKey::Accounts),
            deposits: TreeMap::new(StorageKey::Deposits),
            shares: 0,
            is_minted: false,
            token_whitelist: Lazy::new(StorageKey::TokenWhitelist, token_whitelist),
        }
    }

    pub fn mint(&mut self) {
        require!(!self.is_minted, "Already minted");
        require!(
            env::predecessor_account_id() == self.owner,
            "Only owner can call this function"
        );
        self.shares = NearToken::from_near(1).as_yoctonear();
        self.is_minted = true;
        self.accounts.insert(self.validator.clone(), self.shares);
        FtMint {
            owner_id: &self.validator,
            amount: self.shares.into(),
            memo: None,
        }
        .emit();
    }

    #[payable]
    pub fn burn(&mut self) {
        require!(self.is_minted, "Not yet minted");
        require!(
            env::prepaid_gas()
                >= GAS_FOR_BURN
                    .checked_add(
                        GAS_FOR_FT_TRANSFER
                            .checked_mul(self.deposits.len() as u64)
                            .unwrap()
                    )
                    .unwrap(),
            "Not enough gas attached"
        );
        let sender_id = env::predecessor_account_id();

        let balance = self.accounts.remove(&sender_id).unwrap();

        for (token_id, deposit) in self.deposits.iter_mut() {
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
        *balance = balance.checked_add(amount).unwrap();

        let balance = self.accounts.get_mut(&self.validator).unwrap();
        *balance = balance.checked_sub(amount).unwrap();

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
        env::panic_str("unimplemented");
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
        require!(!self.is_minted, "Already minted");
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
            decimals: 0,
        }
    }
}
