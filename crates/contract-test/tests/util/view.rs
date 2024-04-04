use super::log_view_result;
use near_sdk::json_types::U128;
use near_workspaces::{AccountId, Contract};

pub async fn get_whitelisted_tokens(contract: &Contract) -> anyhow::Result<Vec<AccountId>> {
    let res = log_view_result(
        contract
            .call("get_whitelisted_tokens")
            .max_gas()
            .view()
            .await?,
    )?;
    Ok(res.json()?)
}

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
