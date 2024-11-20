use near_sdk::{env, json_types::Base58CryptoHash, NearToken};
use near_workspaces::{Account, AccountId};
use serde::Serialize;
use tokio::fs;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv()?;

    let worker = near_workspaces::mainnet().await?;
    let reder = Account::from_secret_key(
        "marior.near".parse()?,
        std::env::var("PRIVATE_KEY")?.parse()?,
        &worker,
    );
    let dao_id: AccountId = "shitzu.sputnik-dao.near".parse()?;
    let receiver_id: AccountId = "shit.0xshitzu.near".parse()?;

    let blob = fs::read("./res/contract.wasm").await?;
    // let hash = env::sha256(&blob);
    // let mut blob_hash = [0u8; 32];
    // blob_hash.copy_from_slice(&hash);
    // let hash = Base58CryptoHash::from(blob_hash);
    let storage_cost = ((blob.len() + 32) as u128) * env::storage_byte_cost().as_yoctonear();
    let hash = store_blob(
        &reder,
        &dao_id,
        blob,
        NearToken::from_yoctonear(storage_cost),
    )
    .await?;

    add_proposal(
        &reder,
        &dao_id,
        ProposalInput {
            description:
                "Upgrade contract. This upgrade will make the contract verifiable by sourcescan"
                    .to_string(),
            kind: ProposalKind::UpgradeRemote {
                receiver_id: receiver_id.clone(),
                method_name: "upgrade".to_string(),
                hash,
            },
        },
        None,
    )
    .await?;

    Ok(())
}

pub async fn store_blob(
    sender: &Account,
    dao: &AccountId,
    blob: Vec<u8>,
    storage_cost: NearToken,
) -> anyhow::Result<Base58CryptoHash> {
    Ok(sender
        .call(dao, "store_blob")
        .args(blob)
        .max_gas()
        .deposit(storage_cost)
        .transact()
        .await?
        .json()?)
}

pub async fn add_proposal(
    sender: &Account,
    dao: &AccountId,
    proposal: ProposalInput,
    deposit: Option<NearToken>,
) -> anyhow::Result<u64> {
    Ok(sender
        .call(dao, "add_proposal")
        .args_json((proposal,))
        .max_gas()
        .deposit(deposit.unwrap_or(NearToken::from_millinear(100)))
        .transact()
        .await?
        .json()?)
}
#[derive(Serialize)]
pub struct ProposalInput {
    pub description: String,
    pub kind: ProposalKind,
}

#[derive(Serialize)]
pub enum ProposalKind {
    UpgradeRemote {
        receiver_id: AccountId,
        method_name: String,
        hash: Base58CryptoHash,
    },
}
