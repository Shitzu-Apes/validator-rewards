use super::log_tx_result;
use crate::{
    Action, ActionCall, ContractEvent, DaoConfig, DaoPolicy, FarmingDetails, ProposalInput,
    ProposalKind,
};
use near_contract_standards::non_fungible_token::TokenId;
use near_sdk::{
    json_types::{Base64VecU8, U128},
    serde_json::{self, json},
    Gas,
};
use near_workspaces::{
    result::{ExecutionResult, Value},
    types::NearToken,
    Account, AccountId, Contract,
};

pub async fn storage_deposit(
    contract: &Contract,
    sender: &Account,
    account_id: Option<&AccountId>,
    deposit: Option<NearToken>,
) -> anyhow::Result<ExecutionResult<Value>> {
    let (res, _) = log_tx_result(
        &format!("{} storage_deposit", contract.id()),
        sender
            .call(contract.id(), "storage_deposit")
            .args_json((account_id, None::<bool>))
            .deposit(deposit.unwrap_or(NearToken::from_millinear(50)))
            .max_gas()
            .transact()
            .await?,
    )?;
    Ok(res)
}

pub async fn mint_tokens(
    token: &Contract,
    receiver: &AccountId,
    amount: u128,
) -> anyhow::Result<ExecutionResult<Value>> {
    let (res, _) = log_tx_result(
        &format!("{} mint", token.id()),
        token
            .call("mint")
            .args_json((receiver, U128::from(amount)))
            .transact()
            .await?,
    )?;
    Ok(res)
}

pub async fn propose_add_authorized_farm_token(
    sender: &Account,
    dao: &AccountId,
    pool_id: &AccountId,
    token_id: &AccountId,
) -> anyhow::Result<(u64, Vec<ContractEvent>)> {
    add_proposal(
        "propose_add_authorized_farm_token",
        sender,
        dao,
        ProposalInput {
            description: "".to_string(),
            kind: ProposalKind::FunctionCall {
                receiver_id: pool_id.clone(),
                actions: vec![ActionCall {
                    method_name: "add_authorized_farm_token".to_string(),
                    args: Base64VecU8::from(
                        json!({
                            "token_id": token_id,
                        })
                        .to_string()
                        .as_bytes()
                        .to_vec(),
                    ),
                    deposit: NearToken::from_yoctonear(0),
                    gas: Gas::from_tgas(30),
                }],
            },
        },
        NearToken::from_near(1),
    )
    .await
}

pub async fn propose_deposit_tokens(
    sender: &Account,
    dao: &AccountId,
    token_id: &AccountId,
    receiver_id: &AccountId,
    amount: u128,
) -> anyhow::Result<(u64, Vec<ContractEvent>)> {
    add_proposal(
        "propose_deposit_tokens",
        sender,
        dao,
        ProposalInput {
            description: "".to_string(),
            kind: ProposalKind::FunctionCall {
                receiver_id: token_id.clone(),
                actions: vec![ActionCall {
                    method_name: "ft_transfer_call".to_string(),
                    args: Base64VecU8::from(
                        json!({
                            "receiver_id": receiver_id,
                            "amount": U128(amount),
                            "msg": ""
                        })
                        .to_string()
                        .as_bytes()
                        .to_vec(),
                    ),
                    deposit: NearToken::from_yoctonear(1),
                    gas: Gas::from_tgas(50),
                }],
            },
        },
        NearToken::from_near(1),
    )
    .await
}

pub async fn propose_mint_shares(
    sender: &Account,
    dao: &AccountId,
    contract_id: &AccountId,
    shares: u128,
) -> anyhow::Result<(u64, Vec<ContractEvent>)> {
    add_proposal(
        "propose_mint_shares",
        sender,
        dao,
        ProposalInput {
            description: "".to_string(),
            kind: ProposalKind::FunctionCall {
                receiver_id: contract_id.clone(),
                actions: vec![ActionCall {
                    method_name: "mint".to_string(),
                    args: Base64VecU8::from(
                        json!({
                            "shares": U128(shares)
                        })
                        .to_string()
                        .as_bytes()
                        .to_vec(),
                    ),
                    deposit: NearToken::from_yoctonear(0),
                    gas: Gas::from_tgas(50),
                }],
            },
        },
        NearToken::from_near(1),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn propose_create_farm(
    sender: &Account,
    dao: &AccountId,
    token_id: &AccountId,
    receiver_id: &AccountId,
    amount: u128,
    name: String,
    start_date: u64,
    end_date: u64,
) -> anyhow::Result<(u64, Vec<ContractEvent>)> {
    add_proposal(
        "propose_create_farm",
        sender,
        dao,
        ProposalInput {
            description: "".to_string(),
            kind: ProposalKind::FunctionCall {
                receiver_id: token_id.clone(),
                actions: vec![ActionCall {
                    method_name: "ft_transfer_call".to_string(),
                    args: Base64VecU8::from(
                        json!({
                            "receiver_id": receiver_id,
                            "amount": U128(amount),
                            "msg": serde_json::to_string(&FarmingDetails {
                                name: Some(name),
                                start_date: Some(start_date.into()),
                                end_date: end_date.into(),
                                farm_id: None
                            })?
                        })
                        .to_string()
                        .as_bytes()
                        .to_vec(),
                    ),
                    deposit: NearToken::from_yoctonear(1),
                    gas: Gas::from_tgas(80),
                }],
            },
        },
        NearToken::from_near(1),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn propose_update_farm(
    sender: &Account,
    dao: &AccountId,
    token_id: &AccountId,
    receiver_id: &AccountId,
    amount: u128,
    name: String,
    end_date: u64,
    farm_id: u64,
) -> anyhow::Result<(u64, Vec<ContractEvent>)> {
    add_proposal(
        "propose_update_farm",
        sender,
        dao,
        ProposalInput {
            description: "".to_string(),
            kind: ProposalKind::FunctionCall {
                receiver_id: token_id.clone(),
                actions: vec![ActionCall {
                    method_name: "ft_transfer_call".to_string(),
                    args: Base64VecU8::from(
                        json!({
                            "receiver_id": receiver_id,
                            "amount": U128(amount),
                            "msg": serde_json::to_string(&FarmingDetails {
                                name: Some(name),
                                start_date: None,
                                end_date: end_date.into(),
                                farm_id: Some(farm_id)
                            })?
                        })
                        .to_string()
                        .as_bytes()
                        .to_vec(),
                    ),
                    deposit: NearToken::from_yoctonear(1),
                    gas: Gas::from_tgas(80),
                }],
            },
        },
        NearToken::from_near(1),
    )
    .await
}

pub async fn propose_burn(
    sender: &Account,
    dao: &AccountId,
    contract_id: &AccountId,
) -> anyhow::Result<(u64, Vec<ContractEvent>)> {
    add_proposal(
        "propose_burn",
        sender,
        dao,
        ProposalInput {
            description: "".to_string(),
            kind: ProposalKind::FunctionCall {
                receiver_id: contract_id.clone(),
                actions: vec![ActionCall {
                    method_name: "burn".to_string(),
                    args: Base64VecU8::from(vec![]),
                    deposit: NearToken::from_yoctonear(1),
                    gas: Gas::from_tgas(150),
                }],
            },
        },
        NearToken::from_near(1),
    )
    .await
}

pub async fn propose_withdraw_reward(
    sender: &Account,
    dao: &AccountId,
    contract_id: &AccountId,
    token_id: &AccountId,
    amount: u128,
) -> anyhow::Result<(u64, Vec<ContractEvent>)> {
    add_proposal(
        "propose_withdraw_reward",
        sender,
        dao,
        ProposalInput {
            description: "".to_string(),
            kind: ProposalKind::FunctionCall {
                receiver_id: contract_id.clone(),
                actions: vec![ActionCall {
                    method_name: "withdraw_reward".to_string(),
                    args: Base64VecU8::from(
                        json!({
                            "token_id": token_id,
                            "amount": U128(amount)
                        })
                        .to_string()
                        .as_bytes()
                        .to_vec(),
                    ),
                    deposit: NearToken::from_yoctonear(0),
                    gas: Gas::from_tgas(50),
                }],
            },
        },
        NearToken::from_near(1),
    )
    .await
}

pub async fn propose_remove_reward(
    sender: &Account,
    dao: &AccountId,
    contract_id: &AccountId,
    token_id: &AccountId,
) -> anyhow::Result<(u64, Vec<ContractEvent>)> {
    add_proposal(
        "propose_remove_reward",
        sender,
        dao,
        ProposalInput {
            description: "".to_string(),
            kind: ProposalKind::FunctionCall {
                receiver_id: contract_id.clone(),
                actions: vec![ActionCall {
                    method_name: "remove_reward".to_string(),
                    args: Base64VecU8::from(
                        json!({
                            "token_id": token_id,
                        })
                        .to_string()
                        .as_bytes()
                        .to_vec(),
                    ),
                    deposit: NearToken::from_yoctonear(0),
                    gas: Gas::from_tgas(50),
                }],
            },
        },
        NearToken::from_near(1),
    )
    .await
}

pub async fn new_dao(
    contract: &Contract,
    config: DaoConfig,
    policy: DaoPolicy,
) -> anyhow::Result<(ExecutionResult<Value>, Vec<ContractEvent>)> {
    log_tx_result(
        "DAO: new",
        contract
            .call("new")
            .args_json((config, policy))
            .max_gas()
            .transact()
            .await?,
    )
}

async fn add_proposal(
    ident: &str,
    sender: &Account,
    dao: &AccountId,
    proposal: ProposalInput,
    deposit: NearToken,
) -> anyhow::Result<(u64, Vec<ContractEvent>)> {
    let (res, events) = log_tx_result(
        ident,
        sender
            .call(dao, "add_proposal")
            .args_json((proposal,))
            .max_gas()
            .deposit(deposit)
            .transact()
            .await?,
    )?;
    Ok((res.json()?, events))
}

pub async fn act_proposal(
    sender: &Account,
    dao: &AccountId,
    proposal_id: u64,
    action: Action,
) -> anyhow::Result<(ExecutionResult<Value>, Vec<ContractEvent>)> {
    log_tx_result(
        "DAO: act_proposal",
        sender
            .call(dao, "act_proposal")
            .args_json((proposal_id, action, None::<String>))
            .max_gas()
            .transact()
            .await?,
    )
}

pub async fn deposit_and_stake(
    sender: &Account,
    pool: &AccountId,
    deposit: NearToken,
) -> anyhow::Result<(ExecutionResult<Value>, Vec<ContractEvent>)> {
    log_tx_result(
        "deposit_and_stake",
        sender
            .call(pool, "deposit_and_stake")
            .deposit(deposit)
            .max_gas()
            .transact()
            .await?,
    )
}

pub async fn claim(
    sender: &Account,
    pool: &AccountId,
    token_id: &AccountId,
) -> anyhow::Result<(ExecutionResult<Value>, Vec<ContractEvent>)> {
    log_tx_result(
        "claim",
        sender
            .call(pool, "claim")
            .args_json((token_id, None::<AccountId>))
            .deposit(NearToken::from_yoctonear(1))
            .max_gas()
            .transact()
            .await?,
    )
}

pub async fn burn(
    sender: &Account,
    contract: &AccountId,
) -> anyhow::Result<(U128, Vec<ContractEvent>)> {
    let (res, events) = log_tx_result(
        "burn",
        sender
            .call(contract, "burn")
            .deposit(NearToken::from_yoctonear(1))
            .max_gas()
            .transact()
            .await?,
    )?;
    Ok((res.json()?, events))
}

pub async fn nft_mint(
    sender: &Account,
    nft: &AccountId,
    quantity: u32,
) -> anyhow::Result<(ExecutionResult<Value>, Vec<ContractEvent>)> {
    log_tx_result(
        "Nft: mint",
        sender
            .call(nft, "nft_mint")
            .args_json((quantity,))
            .deposit(NearToken::from_near(5))
            .max_gas()
            .transact()
            .await?,
    )
}

pub async fn stake_nft_with_rewarder(
    sender: &Account,
    nft: &AccountId,
    rewarder: &AccountId,
    token_id: &TokenId,
) -> anyhow::Result<(ExecutionResult<Value>, Vec<ContractEvent>)> {
    log_tx_result(
        "Nft: stake",
        sender
            .call(nft, "nft_transfer_call")
            .args_json((
                rewarder,
                token_id,
                None::<u64>,
                None::<String>,
                "".to_string(),
            ))
            .deposit(NearToken::from_yoctonear(1))
            .max_gas()
            .transact()
            .await?,
    )
}

// async fn ft_transfer_call<T: Serialize>(
//     sender: &Account,
//     token_id: &AccountId,
//     receiver_id: &AccountId,
//     amount: U128,
//     msg: T,
// ) -> anyhow::Result<ExecutionFinalResult> {
//     Ok(sender
//         .call(token_id, "ft_transfer_call")
//         .args_json((
//             receiver_id,
//             amount,
//             Option::<String>::None,
//             json!(msg).to_string(),
//         ))
//         .max_gas()
//         .deposit(NearToken::from_yoctonear(1))
//         .transact()
//         .await?)
// }
