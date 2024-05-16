pub mod call;
pub mod events;
pub mod macros;
pub mod view;

pub use events::*;

use futures::future::join_all;
use near_sdk::{
    json_types::{Base58CryptoHash, Base64VecU8, U128, U64},
    serde::{Deserialize, Serialize},
    serde_json::{self, json},
    AccountId, Gas,
};
use near_workspaces::{
    network::{Sandbox, ValidatorKey},
    result::{ExecutionFinalResult, ExecutionResult, Value, ViewResultDetails},
    types::NearToken,
    Account, Contract, Worker,
};
use owo_colors::OwoColorize;
use std::{
    ops::{Deref, DerefMut},
    path::PathBuf,
    process,
    str::FromStr,
    time::Duration,
};
use tokio::{fs, time::sleep};

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct DaoConfig {
    pub name: String,
    pub purpose: String,
    pub metadata: String,
}

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct DaoPolicy(pub Vec<AccountId>);

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct ProposalInput {
    pub description: String,
    pub kind: ProposalKind,
}

#[allow(unused)]
#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub enum ProposalKind {
    /// Change the DAO config.
    ChangeConfig { config: DaoConfig },
    /// Change the full policy.
    ChangePolicy { policy: DaoPolicy },
    /// Add member to given role in the policy. This is short cut to updating the whole policy.
    AddMemberToRole { member_id: AccountId, role: String },
    /// Remove member to given role in the policy. This is short cut to updating the whole policy.
    RemoveMemberFromRole { member_id: AccountId, role: String },
    /// Calls `receiver_id` with list of method names in a single promise.
    /// Allows this contract to execute any arbitrary set of actions in other contracts.
    FunctionCall {
        receiver_id: AccountId,
        actions: Vec<ActionCall>,
    },
    /// Upgrade this contract with given hash from blob store.
    UpgradeSelf { hash: Base58CryptoHash },
    /// Upgrade another contract, by calling method with the code from given hash from blob store.
    UpgradeRemote {
        receiver_id: AccountId,
        method_name: String,
        hash: Base58CryptoHash,
    },
}

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct ActionCall {
    pub method_name: String,
    pub args: Base64VecU8,
    pub deposit: NearToken,
    pub gas: Gas,
}

#[allow(unused)]
#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub enum Action {
    AddProposal,
    RemoveProposal,
    VoteApprove,
    VoteReject,
    VoteRemove,
    Finalize,
    MoveToHub,
}

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct FarmingDetails {
    pub name: Option<String>,
    pub start_date: Option<U64>,
    pub end_date: U64,
    pub farm_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct HumanReadableFarm {
    pub farm_id: u64,
    pub name: String,
    pub token_id: AccountId,
    pub amount: U128,
    pub start_date: U64,
    pub end_date: U64,
    pub active: bool,
}

#[derive(Debug, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct HumanReadableAccount {
    pub account_id: AccountId,
    pub unstaked_balance: U128,
    pub staked_balance: U128,
    pub can_withdraw: bool,
}

pub struct Init {
    pub worker: Worker<Sandbox>,
    pub near: Account,
    pub council: Account,
    pub contract: Contract,
    pub dao_contract: Contract,
    pub pool_contract: Contract,
    pub nft_contract: Contract,
    pub token_contracts: Vec<Contract>,
}

pub struct NeardProcess(process::Child);

impl Drop for NeardProcess {
    fn drop(&mut self) {
        std::fs::remove_dir_all("../../.near/data").unwrap();
        self.0.wait().unwrap();
    }
}

impl Deref for NeardProcess {
    type Target = process::Child;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for NeardProcess {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub async fn initialize_blockchain() -> anyhow::Result<NeardProcess> {
    let neard = NeardProcess(
        process::Command::new("../../res/near-sandbox")
            .args(["--home", "../../.near", "run"])
            .spawn()?,
    );
    sleep(Duration::from_secs(8)).await;
    Ok(neard)
}

pub async fn initialize_contracts() -> anyhow::Result<Init> {
    let worker = near_workspaces::sandbox()
        .rpc_addr("http://localhost:3030")
        .validator_key(ValidatorKey::HomeDir(PathBuf::from_str("../../.near")?))
        .await?;

    let near = Account::from_file("../../.near/near.json", &worker)?;
    let council = near
        .create_subaccount("council")
        .initial_balance(NearToken::from_near(100_000))
        .transact()
        .await?
        .into_result()?;

    let dao_contract = near
        .create_subaccount("dao")
        .initial_balance(NearToken::from_near(100_000))
        .transact()
        .await?
        .into_result()?
        .deploy(&fs::read("../../res/sputnik_dao.wasm").await?)
        .await?
        .into_result()?;
    call::new_dao(
        &dao_contract,
        DaoConfig {
            name: "Shitzu DAO".to_string(),
            purpose: "memes".to_string(),
            metadata: "".to_string(),
        },
        DaoPolicy(vec![council.id().clone()]),
    )
    .await?;

    let pool_contract = Account::from_file("../../.near/shitzu.pool.near.json", &worker)?
        .deploy(&fs::read("../../res/pool.wasm").await?)
        .await?
        .into_result()?;
    log_tx_result(
        "Pool: new",
        pool_contract
            .call("new")
            .args_json(json!({
                "owner_id": dao_contract.id(),
                "stake_public_key": pool_contract.as_account().secret_key().public_key(),
                "reward_fee_fraction": {
                    "numerator": 1,
                    "denominator": 2
                },
                "burn_fee_fraction": {
                    "numerator": 0,
                    "denominator": 100
                }
            }))
            .max_gas()
            .transact()
            .await?,
    )?;

    let nft_contract = near
        .create_subaccount("nft")
        .initial_balance(NearToken::from_near(100_000))
        .transact()
        .await?
        .into_result()?
        .deploy(&fs::read("../../res/shitzu_nft.wasm").await?)
        .await?
        .into_result()?;
    log_tx_result(
        "Nft: new",
        nft_contract
            .call("new_init")
            .args_json(json!({
                "owner_id": dao_contract.id(),
                "icon": ""
            }))
            .max_gas()
            .transact()
            .await?,
    )?;

    let token_contracts: Vec<_> = join_all((0..3).map(|i| {
        let near = near.clone();
        tokio::spawn(async move {
            initialize_token(
                &near,
                &format!("Token {}", i),
                &format!("TKN{}", i),
                None,
                18,
            )
            .await
            .unwrap()
        })
    }))
    .await
    .into_iter()
    .map(|c| c.unwrap())
    .collect();

    let contract = near
        .create_subaccount("contract")
        .initial_balance(NearToken::from_near(100_000))
        .transact()
        .await?
        .into_result()?
        .deploy(&fs::read("../../res/contract.wasm").await?)
        .await?
        .into_result()?;

    log_tx_result(
        "new",
        contract
            .call("new")
            .args_json((
                dao_contract.id(),
                pool_contract.id(),
                nft_contract.id(),
                token_contracts
                    .iter()
                    .map(|contract| contract.id())
                    .collect::<Vec<_>>(),
            ))
            .max_gas()
            .transact()
            .await?,
    )?;

    Ok(Init {
        worker,
        near,
        council,
        contract,
        dao_contract,
        pool_contract,
        nft_contract,
        token_contracts,
    })
}

pub async fn initialize_token(
    near: &Account,
    name: &str,
    ticker: &str,
    icon: Option<&str>,
    decimals: u8,
) -> anyhow::Result<Contract> {
    let token_contract = near
        .create_subaccount(&ticker.to_lowercase())
        .initial_balance(NearToken::from_near(100))
        .transact()
        .await?
        .into_result()?
        .deploy(&fs::read("../../res/test_token.wasm").await?)
        .await?
        .into_result()?;
    log_tx_result(
        &format!("Token {}: new", ticker),
        token_contract
            .call("new")
            .args_json((name, ticker, icon, decimals))
            .transact()
            .await?,
    )?;

    Ok(token_contract)
}

pub fn log_tx_result(
    ident: &str,
    res: ExecutionFinalResult,
) -> anyhow::Result<(ExecutionResult<Value>, Vec<ContractEvent>)> {
    for failure in res.receipt_failures() {
        println!("{:#?}", failure.bright_red());
    }
    let mut events = vec![];
    for outcome in res.receipt_outcomes() {
        if !outcome.logs.is_empty() {
            for log in outcome.logs.iter() {
                if log.starts_with("EVENT_JSON:") {
                    if let Ok(event) =
                        serde_json::from_str::<ContractEvent>(&log.replace("EVENT_JSON:", ""))
                    {
                        events.push(event.clone());
                        println!(
                            "{}: {}\n{}",
                            "account".bright_cyan(),
                            outcome.executor_id,
                            event
                        );
                    }
                } else {
                    println!("{}", log.bright_yellow());
                }
            }
        }
    }
    println!(
        "{} gas burnt: {:.3} {}",
        ident.italic(),
        res.total_gas_burnt.as_tgas().bright_magenta().bold(),
        "TGas".bright_magenta().bold()
    );
    Ok((res.into_result()?, events))
}

pub fn log_view_result(res: ViewResultDetails) -> anyhow::Result<ViewResultDetails> {
    if !res.logs.is_empty() {
        for log in res.logs.iter() {
            println!("{}", log.bright_yellow());
        }
    }
    Ok(res)
}

// pub fn assert_event_emits<T>(actual: T, events: Vec<ContractEvent>) -> anyhow::Result<()>
// where
//     T: Serialize + fmt::Debug + Clone,
// {
//     let actual = serde_json::to_value(&actual)?;
//     let mut expected = vec![];
//     for event in events {
//         let mut expected_event = serde_json::to_value(event)?;
//         let ev = expected_event.as_object_mut().unwrap();
//         let event_str = ev.get("event").unwrap().as_str().unwrap();
//         ev.insert("standard".into(), "chess-game".into());
//         expected.push(expected_event);
//     }
//     assert_eq!(
//         &actual,
//         &serde_json::to_value(&expected)?,
//         "actual and expected events did not match.\nActual: {:#?}\nExpected: {:#?}",
//         &actual,
//         &expected
//     );
//     Ok(())
// }

// pub fn assert_ft_mint_events<T>(actual: T, events: Vec<FtMint>) -> anyhow::Result<()>
// where
//     T: Serialize + fmt::Debug + Clone,
// {
//     let mut actual = serde_json::to_value(&actual)?;
//     actual.as_array_mut().unwrap().retain(|ac| {
//         let event_str = ac
//             .as_object()
//             .unwrap()
//             .get("event")
//             .unwrap()
//             .as_str()
//             .unwrap();
//         event_str == "ft_mint"
//     });
//     let mut expected = vec![];
//     for event in events {
//         expected.push(json!({
//             "event": "ft_mint",
//             "standard": "nep141",
//             "version": "1.0.0",
//             "data": [event]
//         }));
//     }
//     assert_eq!(
//         &actual,
//         &serde_json::to_value(&expected)?,
//         "actual and expected events did not match.\nActual: {:#?}\nExpected: {:#?}",
//         &actual,
//         &expected
//     );
//     Ok(())
// }
