pub mod call;
pub mod events;
pub mod macros;
pub mod view;

pub use events::*;

use near_sdk::{
    env,
    json_types::{Base58CryptoHash, Base64VecU8, U64},
    serde::Serialize,
    serde_json::{self, json},
    AccountId, Gas,
};
use near_workspaces::{
    network::Sandbox,
    result::{ExecutionFinalResult, ExecutionResult, Value, ViewResultDetails},
    types::{KeyType, NearToken, SecretKey},
    Account, Contract, Worker,
};
use owo_colors::OwoColorize;
use tokio::fs;

#[macro_export]
macro_rules! print_log {
    ( $x:expr, $($y:expr),+ ) => {
        let thread_name = std::thread::current().name().unwrap().to_string();
        if thread_name == "main" {
            println!($x, $($y),+);
        } else {
            let mut s = format!($x, $($y),+);
            s = s.split('\n').map(|s| {
                let mut pre = "    ".to_string();
                pre.push_str(s);
                pre.push('\n');
                pre
            }).collect::<String>();
            println!(
                "{}\n{}",
                thread_name.bold(),
                &s[..s.len() - 1],
            );
        }
    };
}

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

pub struct Init {
    pub worker: Worker<Sandbox>,
    pub council: Account,
    pub contract: Contract,
    pub dao_contract: Contract,
    pub pool_contract: Contract,
    pub token_contracts: Vec<Contract>,
}

pub async fn initialize_contracts() -> anyhow::Result<Init> {
    let worker = near_workspaces::sandbox().await?;

    let council = worker.dev_create_account().await?;

    let key = SecretKey::from_random(KeyType::ED25519);
    let dao_contract = worker
        .create_tla_and_deploy(
            "dao.test.near".parse()?,
            key,
            &fs::read("../../res/sputnik_dao.wasm").await?,
        )
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

    let key = SecretKey::from_random(KeyType::ED25519);
    let whitelist_contract = worker
        .create_tla_and_deploy(
            "whitelist.test.near".parse()?,
            key,
            &fs::read("../../res/lockup_whitelist.wasm").await?,
        )
        .await?
        .into_result()?;
    log_tx_result(
        "Whitelist: new",
        whitelist_contract
            .call("new")
            .args_json(json!({
              "foundation_account_id": council.id()
            }))
            .max_gas()
            .transact()
            .await?,
    )?;

    let key = SecretKey::from_random(KeyType::ED25519);
    let pool_factory_contract = worker
        .create_tla_and_deploy(
            "pool.test.near".parse()?,
            key,
            &fs::read("../../res/pool_factory.wasm").await?,
        )
        .await?
        .into_result()?;
    log_tx_result(
        "Pool: new",
        pool_factory_contract
            .call("new")
            .args_json(json!({
                "owner_id": council.id(),
                "staking_pool_whitelist_account_id": whitelist_contract.id()
            }))
            .max_gas()
            .transact()
            .await?,
    )?;

    let blob = fs::read("../../res/pool.wasm").await?;
    let storage_cost = env::storage_byte_cost()
        .checked_mul((blob.len() + 128) as u128)
        .unwrap();
    let (res, _) = log_tx_result(
        "Pool: store",
        pool_factory_contract
            .call("store")
            .args(blob)
            .max_gas()
            .deposit(storage_cost)
            .transact()
            .await?,
    )?;
    let code_hash: String = res.json()?;
    log_tx_result(
        "Pool: allow_contract",
        council
            .call(pool_factory_contract.id(), "allow_contract")
            .args_json((code_hash.clone(),))
            .max_gas()
            .transact()
            .await?,
    )?;
    let _ = pool_factory_contract
        .call("create_staking_pool")
        .args_json(json!({
          "code_hash": code_hash,
          "owner_id": dao_contract.id(),
          "reward_fee_fraction": {
            "denominator": 2,
            "numerator": 1
          },
          "stake_public_key": "ed25519:63vV68WsFzuKSFfYzr5Z5HzrTzBkERHGQ1iiox4krugg",
          "staking_pool_id": "shitzu"
        }))
        .max_gas()
        .deposit(NearToken::from_near(5))
        .transact()
        .await?;

    let mut token_contracts = vec![];
    for i in 0..3 {
        let token_contract = initialize_token(
            &worker,
            &format!("Token {}", i),
            &format!("TKN{}", i),
            None,
            18,
        )
        .await?;
        token_contracts.push(token_contract);
    }

    let key = SecretKey::from_random(KeyType::ED25519);
    let contract = worker
        .create_tla_and_deploy(
            "contract.test.near".parse()?,
            key,
            &fs::read("../../res/contract.wasm").await?,
        )
        .await?
        .into_result()?;

    log_tx_result(
        "new",
        contract
            .call("new")
            .args_json((
                dao_contract.id(),
                pool_factory_contract.id(),
                token_contracts
                    .iter()
                    .map(|contract| contract.id())
                    .collect::<Vec<_>>(),
            ))
            .max_gas()
            .transact()
            .await?,
    )?;

    let pool_contract = Contract::from_secret_key(
        format!("shitzu.{}", pool_factory_contract.id()).parse()?,
        SecretKey::from_random(KeyType::ED25519),
        &worker,
    );

    Ok(Init {
        worker,
        council,
        contract,
        dao_contract,
        pool_contract,
        token_contracts,
    })
}

pub async fn initialize_token(
    worker: &Worker<Sandbox>,
    name: &str,
    ticker: &str,
    icon: Option<&str>,
    decimals: u8,
) -> anyhow::Result<Contract> {
    let key = SecretKey::from_random(KeyType::ED25519);
    let token_contract = worker
        .create_tla_and_deploy(
            format!("{}.test.near", ticker.to_lowercase()).parse()?,
            key,
            &fs::read("../../res/test_token.wasm").await?,
        )
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
        print_log!("{:#?}", failure.bright_red());
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
                        print_log!(
                            "{}: {}\n{}",
                            "account".bright_cyan(),
                            outcome.executor_id,
                            event
                        );
                    }
                } else {
                    print_log!("{}", log.bright_yellow());
                }
            }
        }
    }
    print_log!(
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
            print_log!("{}", log.bright_yellow());
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
