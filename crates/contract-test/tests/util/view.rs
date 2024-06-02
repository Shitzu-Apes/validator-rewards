use super::log_view_result;
use crate::{HumanReadableAccount, HumanReadableFarm};
use near_contract_standards::non_fungible_token::{Token, TokenId};
use near_sdk::json_types::U128;
use near_workspaces::{AccountId, Contract};

pub async fn get_undistributed_rewards(
    contract: &Contract,
) -> anyhow::Result<Vec<(AccountId, U128)>> {
    let res = log_view_result(
        contract
            .call("get_undistributed_rewards")
            .max_gas()
            .view()
            .await?,
    )?;
    Ok(res.json()?)
}

pub async fn get_deposits(contract: &Contract) -> anyhow::Result<Vec<(AccountId, U128)>> {
    let res = log_view_result(contract.call("get_deposits").max_gas().view().await?)?;
    Ok(res.json()?)
}

pub async fn get_farm(contract: &Contract, farm_id: u64) -> anyhow::Result<HumanReadableFarm> {
    let res = log_view_result(
        contract
            .call("get_farm")
            .args_json((farm_id,))
            .max_gas()
            .view()
            .await?,
    )?;
    Ok(res.json()?)
}

pub async fn get_account(
    contract: &Contract,
    account_id: &AccountId,
) -> anyhow::Result<HumanReadableAccount> {
    let res = log_view_result(
        contract
            .call("get_account")
            .args_json((account_id,))
            .max_gas()
            .view()
            .await?,
    )?;
    Ok(res.json()?)
}

pub async fn get_unclaimed_reward(
    contract: &Contract,
    account_id: &AccountId,
    farm_id: u64,
) -> anyhow::Result<U128> {
    let res = log_view_result(
        contract
            .call("get_unclaimed_reward")
            .args_json((account_id, farm_id))
            .max_gas()
            .view()
            .await?,
    )?;
    Ok(res.json()?)
}

pub async fn ft_balance_of(contract: &Contract, account_id: &AccountId) -> anyhow::Result<U128> {
    let res = log_view_result(
        contract
            .call("ft_balance_of")
            .args_json((account_id,))
            .max_gas()
            .view()
            .await?,
    )?;
    Ok(res.json()?)
}

pub async fn ft_total_supply(contract: &Contract) -> anyhow::Result<U128> {
    let res = log_view_result(contract.call("ft_total_supply").max_gas().view().await?)?;
    Ok(res.json()?)
}

pub async fn nft_tokens_for_owner(
    contract: &Contract,
    account_id: &AccountId,
) -> anyhow::Result<Vec<Token>> {
    let res: near_workspaces::result::ViewResultDetails = contract
        .call("nft_tokens_for_owner")
        .args_json((account_id, None::<U128>, None::<u64>))
        .view()
        .await?;

    Ok(res.json::<Vec<Token>>()?)
}

pub async fn primary_nft_of(
    contract: &Contract,
    account_id: &AccountId,
) -> anyhow::Result<Option<(TokenId, U128)>> {
    let res = contract
        .call("primary_nft_of")
        .args_json((account_id,))
        .view()
        .await?;

    Ok(res.json::<_>()?)
}
