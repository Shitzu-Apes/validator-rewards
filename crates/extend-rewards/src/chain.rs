use crate::proposal::ProposalInput;
use anyhow::{bail, Context, Result};
use near_sdk::{
    json_types::{U128, U64},
    AccountId,
};
use near_workspaces::{
    network::Mainnet,
    types::{NearToken, SecretKey},
    Account, Worker,
};
use serde::{de::DeserializeOwned, Deserialize};

#[derive(Deserialize, Clone, Debug)]
pub struct FtMetadata {
    pub symbol: String,
    pub decimals: u8,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Farm {
    pub farm_id: u64,
    pub name: String,
    pub token_id: AccountId,
    pub amount: U128,
    pub start_date: U64,
    pub end_date: U64,
    pub active: bool,
}

pub async fn view<T: DeserializeOwned>(
    worker: &Worker<Mainnet>,
    contract: &AccountId,
    method: &str,
    args: serde_json::Value,
) -> Result<T> {
    let result = worker
        .view(contract, method)
        .args_json(args)
        .await
        .with_context(|| format!("failed to call view `{contract}.{method}`"))?;
    result
        .json()
        .with_context(|| format!("failed to parse `{contract}.{method}` response"))
}

pub async fn ft_metadata(worker: &Worker<Mainnet>, token: &AccountId) -> Result<FtMetadata> {
    view(worker, token, "ft_metadata", serde_json::json!({})).await
}

pub async fn ft_balance_of(
    worker: &Worker<Mainnet>,
    token: &AccountId,
    account: &AccountId,
) -> Result<u128> {
    Ok(view::<U128>(
        worker,
        token,
        "ft_balance_of",
        serde_json::json!({ "account_id": account }),
    )
    .await?
    .0)
}

pub async fn storage_balance_of(
    worker: &Worker<Mainnet>,
    token: &AccountId,
    account: &AccountId,
) -> Result<Option<serde_json::Value>> {
    view(
        worker,
        token,
        "storage_balance_of",
        serde_json::json!({ "account_id": account }),
    )
    .await
}

pub async fn whitelisted_tokens(
    worker: &Worker<Mainnet>,
    dogshit: &AccountId,
) -> Result<Vec<AccountId>> {
    view(
        worker,
        dogshit,
        "get_whitelisted_tokens",
        serde_json::json!({}),
    )
    .await
}

pub async fn deposits(
    worker: &Worker<Mainnet>,
    dogshit: &AccountId,
) -> Result<Vec<(AccountId, U128)>> {
    view(worker, dogshit, "get_deposits", serde_json::json!({})).await
}

pub async fn rewards(
    worker: &Worker<Mainnet>,
    dogshit: &AccountId,
) -> Result<Vec<(AccountId, U128)>> {
    view(
        worker,
        dogshit,
        "get_undistributed_rewards",
        serde_json::json!({}),
    )
    .await
}

pub async fn total_supply(worker: &Worker<Mainnet>, dogshit: &AccountId) -> Result<u128> {
    Ok(
        view::<U128>(worker, dogshit, "ft_total_supply", serde_json::json!({}))
            .await?
            .0,
    )
}

pub async fn total_staked_balance(worker: &Worker<Mainnet>, pool: &AccountId) -> Result<u128> {
    Ok(view::<U128>(
        worker,
        pool,
        "get_total_staked_balance",
        serde_json::json!({}),
    )
    .await?
    .0)
}

pub async fn get_farm(worker: &Worker<Mainnet>, pool: &AccountId, farm_id: u64) -> Result<Farm> {
    view(
        worker,
        pool,
        "get_farm",
        serde_json::json!({ "farm_id": farm_id }),
    )
    .await
}

pub async fn add_proposal(
    worker: &Worker<Mainnet>,
    signer_id: AccountId,
    secret_key: SecretKey,
    dao: &AccountId,
    proposal: &ProposalInput,
    bond: NearToken,
) -> Result<u64> {
    let signer = Account::from_secret_key(signer_id, secret_key, worker);
    let result = signer
        .call(dao, "add_proposal")
        .args_json(add_proposal_args(proposal))
        .max_gas()
        .deposit(bond)
        .transact()
        .await
        .context("failed to send add_proposal transaction")?;
    if result.is_failure() {
        for failure in result.failures() {
            eprintln!("receipt failure: {failure:?}");
        }
        bail!("add_proposal transaction failed");
    }
    result.json().context("failed to parse proposal id")
}

/// sputnikdao2 generates named arguments for `add_proposal(proposal)`, so the
/// JSON must be `{"proposal": {...}}` rather than the bare `ProposalInput`.
fn add_proposal_args(proposal: &ProposalInput) -> serde_json::Value {
    serde_json::json!({ "proposal": proposal })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proposal::mint_proposal;

    #[test]
    fn add_proposal_args_are_wrapped_under_proposal() {
        let dogshit: AccountId = "shit.0xshitzu.near".parse().unwrap();
        let proposal = mint_proposal(&dogshit, 1, "test".to_string()).unwrap();
        let args = add_proposal_args(&proposal);
        assert_eq!(args["proposal"]["description"], "test");
        assert!(args.get("description").is_none());
    }
}
