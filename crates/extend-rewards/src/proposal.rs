use anyhow::{bail, Context, Result};
use chrono::{DateTime, NaiveDate};
use near_sdk::{
    json_types::{Base64VecU8, U128, U64},
    AccountId,
};
use serde::Serialize;

/// Gas attached to every action inside a DAO proposal. Matches the values used
/// by the existing proposals (#426, #429, #430).
pub const ACTION_GAS: &str = "270000000000000";
/// `remove_reward` only needs to trigger one FT transfer back to the DAO.
pub const REMOVE_REWARD_GAS: &str = "50000000000000";

#[derive(Serialize, Clone, Debug)]
pub struct ProposalInput {
    pub description: String,
    pub kind: ProposalKind,
}

#[derive(Serialize, Clone, Debug)]
pub enum ProposalKind {
    FunctionCall {
        receiver_id: AccountId,
        actions: Vec<ActionCall>,
    },
}

#[derive(Serialize, Clone, Debug)]
pub struct ActionCall {
    pub method_name: String,
    pub args: Base64VecU8,
    pub deposit: String,
    pub gas: String,
}

#[derive(Serialize)]
struct FtTransferCallArgs {
    receiver_id: AccountId,
    amount: U128,
    msg: String,
}

#[derive(Serialize)]
struct MintArgs {
    shares: U128,
}

#[derive(Serialize)]
struct RemoveRewardArgs {
    token_id: AccountId,
}

#[derive(Serialize)]
struct FarmingDetails {
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_date: Option<U64>,
    end_date: U64,
    #[serde(skip_serializing_if = "Option::is_none")]
    farm_id: Option<u64>,
}

fn function_call<T: Serialize>(
    receiver_id: AccountId,
    method_name: &str,
    args: &T,
    deposit: &str,
    gas: &str,
    description: String,
) -> Result<ProposalInput> {
    let args = serde_json::to_vec(args).context("failed to serialize action args")?;
    Ok(ProposalInput {
        description,
        kind: ProposalKind::FunctionCall {
            receiver_id,
            actions: vec![ActionCall {
                method_name: method_name.to_string(),
                args: Base64VecU8::from(args),
                deposit: deposit.to_string(),
                gas: gas.to_string(),
            }],
        },
    })
}

/// Step 1: send `amount` of `token` from the DAO to the Dogshit contract.
pub fn transfer_proposal(
    token: &AccountId,
    dogshit: &AccountId,
    amount: u128,
    description: String,
) -> Result<ProposalInput> {
    function_call(
        token.clone(),
        "ft_transfer_call",
        &FtTransferCallArgs {
            receiver_id: dogshit.clone(),
            amount: U128(amount),
            msg: String::new(),
        },
        "1",
        ACTION_GAS,
        description,
    )
}

/// Step 2: mint DOGSHIT shares. This wraps every pending deposit.
pub fn mint_proposal(
    dogshit: &AccountId,
    shares: u128,
    description: String,
) -> Result<ProposalInput> {
    function_call(
        dogshit.clone(),
        "mint",
        &MintArgs {
            shares: U128(shares),
        },
        "0",
        ACTION_GAS,
        description,
    )
}

/// Remove a reward token from the Dogshit contract and send it to the DAO.
pub fn remove_reward_proposal(
    dogshit: &AccountId,
    token: &AccountId,
    description: String,
) -> Result<ProposalInput> {
    function_call(
        dogshit.clone(),
        "remove_reward",
        &RemoveRewardArgs {
            token_id: token.clone(),
        },
        "0",
        REMOVE_REWARD_GAS,
        description,
    )
}

/// Step 3: send DOGSHIT shares to the pool and update the farm end date.
#[allow(clippy::too_many_arguments)]
pub fn farm_proposal(
    dogshit: &AccountId,
    pool: &AccountId,
    amount: u128,
    name: &str,
    farm_id: u64,
    end_date: u64,
    description: String,
) -> Result<ProposalInput> {
    let msg = serde_json::to_string(&FarmingDetails {
        name: Some(name.to_string()),
        start_date: None,
        end_date: U64(end_date),
        farm_id: Some(farm_id),
    })
    .context("failed to serialize farming details")?;
    function_call(
        dogshit.clone(),
        "ft_transfer_call",
        &FtTransferCallArgs {
            receiver_id: pool.clone(),
            amount: U128(amount),
            msg,
        },
        "1",
        ACTION_GAS,
        description,
    )
}

/// Convert a human-readable decimal amount into raw token units without floats.
pub fn parse_units(input: &str, decimals: u8) -> Result<u128> {
    let input = input.trim().replace('_', "");
    if input.is_empty() {
        bail!("amount is empty");
    }
    if input.starts_with('-') {
        bail!("amount must not be negative");
    }
    let input = input.strip_prefix('+').unwrap_or(&input);

    let mut parts = input.split('.');
    let whole = parts.next().unwrap_or_default();
    let fractional = parts.next();
    if parts.next().is_some() {
        bail!("amount contains more than one decimal point");
    }
    if whole.is_empty() && fractional.is_none_or(str::is_empty) {
        bail!("amount is empty");
    }
    if !whole.chars().all(|c| c.is_ascii_digit())
        || !fractional
            .unwrap_or_default()
            .chars()
            .all(|c| c.is_ascii_digit())
    {
        bail!("amount must contain only digits and an optional decimal point");
    }

    let factor = 10u128
        .checked_pow(decimals as u32)
        .context("token decimals are too large")?;
    let whole: u128 = if whole.is_empty() {
        0
    } else {
        whole.parse().context("amount is too large")?
    };
    let mut value = whole
        .checked_mul(factor)
        .context("amount is too large for u128")?;

    if let Some(fractional) = fractional {
        if fractional.len() > decimals as usize {
            bail!("amount has more than {decimals} decimal places");
        }
        let padded = format!("{:0<width$}", fractional, width = decimals as usize);
        let fractional: u128 = if padded.is_empty() {
            0
        } else {
            padded.parse().context("invalid fractional amount")?
        };
        value = value
            .checked_add(fractional)
            .context("amount is too large for u128")?;
    }

    Ok(value)
}

/// Accepts YYYY-MM-DD (UTC midnight), RFC3339, seconds, or nanoseconds.
pub fn parse_end_date(input: &str) -> Result<u64> {
    let input = input.trim();
    if input.is_empty() {
        bail!("end date is empty");
    }

    if input.chars().all(|c| c.is_ascii_digit()) {
        let value: u64 = input.parse().context("invalid timestamp")?;
        return if value >= 1_000_000_000_000_000 {
            Ok(value)
        } else if value >= 1_000_000_000 {
            value
                .checked_mul(1_000_000_000)
                .context("timestamp is too large")
        } else {
            bail!("timestamp must be at least 1e9 seconds or 1e15 nanoseconds")
        };
    }

    if let Ok(date) = DateTime::parse_from_rfc3339(input) {
        let nanos = date.timestamp_nanos_opt().context("date is out of range")?;
        return u64::try_from(nanos).context("end date must be after 1970-01-01");
    }

    if let Ok(date) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        let datetime = date.and_hms_opt(0, 0, 0).context("invalid date")?.and_utc();
        let nanos = datetime
            .timestamp_nanos_opt()
            .context("date is out of range")?;
        return u64::try_from(nanos).context("end date must be after 1970-01-01");
    }

    bail!("could not parse end date `{input}`; use YYYY-MM-DD, RFC3339, seconds, or nanoseconds")
}

pub fn default_transfer_description(amount: &str, symbol: &str) -> String {
    format!("Deposit {amount} ${symbol} into Dogshit contract for validator rewards")
}

pub fn default_mint_description(shares: &str) -> String {
    format!(
        "Mint $DOGSHIT. Wraps all deposited FTs into {shares} dogshit shares for validator distribution"
    )
}

pub fn default_farm_description(end_date: &str) -> String {
    let date = end_date.split('T').next().unwrap_or(end_date);
    if date.contains('-') {
        format!(
            "Deposit all $DOGSHIT shares into validator pool contract to extend farm until {date} 0am UTC"
        )
    } else {
        format!(
            "Deposit all $DOGSHIT shares into validator pool contract to extend farm until {date}"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn action_args(proposal: &ProposalInput) -> Value {
        let ProposalKind::FunctionCall { actions, .. } = &proposal.kind;
        serde_json::from_slice(&actions[0].args.0).expect("args are valid json")
    }

    #[test]
    fn parses_human_amounts() {
        assert_eq!(
            parse_units("300000", 18).unwrap(),
            300_000_000_000_000_000_000_000
        );
        assert_eq!(parse_units("1000000", 8).unwrap(), 100_000_000_000_000);
        assert_eq!(
            parse_units("60", 24).unwrap(),
            60_000_000_000_000_000_000_000_000
        );
        assert_eq!(parse_units("1.5", 1).unwrap(), 15);
        assert_eq!(parse_units("0.000001", 6).unwrap(), 1);
        assert_eq!(parse_units(".5", 1).unwrap(), 5);
        assert_eq!(parse_units("5.", 2).unwrap(), 500);
        assert!(parse_units("1.234", 2).is_err());
        assert!(parse_units("-1", 2).is_err());
        assert!(parse_units("", 2).is_err());
        assert!(parse_units("1.2.3", 2).is_err());
    }

    #[test]
    fn parses_end_dates() {
        let expected = 1_792_886_400_000_000_000;
        assert_eq!(parse_end_date("2026-10-25").unwrap(), expected);
        assert_eq!(parse_end_date("2026-10-25T00:00:00Z").unwrap(), expected);
        assert_eq!(parse_end_date("1792886400000000000").unwrap(), expected);
        assert_eq!(parse_end_date("1792886400").unwrap(), expected);
        assert!(parse_end_date("2026-13-01").is_err());
        assert!(parse_end_date("123").is_err());
    }

    #[test]
    fn builds_example_deposit_proposal() {
        let token: AccountId = "token.0xshitzu.near".parse().unwrap();
        let dogshit: AccountId = "shit.0xshitzu.near".parse().unwrap();
        let amount = parse_units("300000", 18).unwrap();
        let proposal = transfer_proposal(&token, &dogshit, amount, "deposit".to_string()).unwrap();

        assert_eq!(
            serde_json::to_value(&proposal).unwrap(),
            json!({
                "description": "deposit",
                "kind": {
                    "FunctionCall": {
                        "receiver_id": "token.0xshitzu.near",
                        "actions": [{
                            "method_name": "ft_transfer_call",
                            "args": "eyJyZWNlaXZlcl9pZCI6InNoaXQuMHhzaGl0enUubmVhciIsImFtb3VudCI6IjMwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMCIsIm1zZyI6IiJ9",
                            "deposit": "1",
                            "gas": ACTION_GAS
                        }]
                    }
                }
            })
        );
        assert_eq!(
            action_args(&proposal),
            json!({
                "receiver_id": "shit.0xshitzu.near",
                "amount": "300000000000000000000000",
                "msg": ""
            })
        );
    }

    #[test]
    fn builds_example_mint_proposal() {
        let dogshit: AccountId = "shit.0xshitzu.near".parse().unwrap();
        let shares = parse_units("600000", 24).unwrap();
        let proposal = mint_proposal(&dogshit, shares, "mint".to_string()).unwrap();

        assert_eq!(
            action_args(&proposal),
            json!({ "shares": "600000000000000000000000000000" })
        );
        let ProposalKind::FunctionCall {
            receiver_id,
            actions,
        } = &proposal.kind;
        assert_eq!(receiver_id, &dogshit);
        assert_eq!(actions[0].method_name, "mint");
        assert_eq!(actions[0].deposit, "0");
    }

    #[test]
    fn builds_remove_reward_proposal() {
        let dogshit: AccountId = "shit.0xshitzu.near".parse().unwrap();
        let token: AccountId = "4illia-222.meme-cooking.near".parse().unwrap();
        let proposal = remove_reward_proposal(&dogshit, &token, "remove".to_string()).unwrap();

        assert_eq!(
            action_args(&proposal),
            json!({ "token_id": "4illia-222.meme-cooking.near" })
        );
        let ProposalKind::FunctionCall {
            receiver_id,
            actions,
        } = &proposal.kind;
        assert_eq!(receiver_id, &dogshit);
        assert_eq!(actions[0].method_name, "remove_reward");
        assert_eq!(actions[0].deposit, "0");
        assert_eq!(actions[0].gas, REMOVE_REWARD_GAS);
    }

    #[test]
    fn builds_example_farm_proposal() {
        let dogshit: AccountId = "shit.0xshitzu.near".parse().unwrap();
        let pool: AccountId = "shitzu.pool.near".parse().unwrap();
        let amount = parse_units("600000", 24).unwrap();
        let end_date = parse_end_date("2026-10-25").unwrap();
        let proposal = farm_proposal(
            &dogshit,
            &pool,
            amount,
            "Dogshit",
            0,
            end_date,
            "farm".to_string(),
        )
        .unwrap();

        let args = action_args(&proposal);
        assert_eq!(args["receiver_id"], json!("shitzu.pool.near"));
        assert_eq!(args["amount"], json!("600000000000000000000000000000"));
        let msg: Value = serde_json::from_str(args["msg"].as_str().unwrap()).unwrap();
        assert_eq!(
            msg,
            json!({
                "name": "Dogshit",
                "end_date": "1792886400000000000",
                "farm_id": 0
            })
        );
    }
}
