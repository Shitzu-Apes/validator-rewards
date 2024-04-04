mod util;

use near_sdk::json_types::U128;
use util::*;

#[tokio::test]
async fn test_all() -> anyhow::Result<()> {
    let Init {
        worker,
        council,
        contract,
        dao_contract,
        pool_contract,
        token_contracts,
    } = initialize_contracts().await?;

    let default_amount = 1_000_000;

    let (proposal_id, _) = call::propose_add_authorized_farm_token(
        &council,
        dao_contract.id(),
        pool_contract.id(),
        contract.id(),
    )
    .await?;
    call::act_proposal(
        &council,
        dao_contract.id(),
        proposal_id,
        Action::VoteApprove,
    )
    .await?;

    for token_contract in &token_contracts {
        call::storage_deposit(token_contract, &council, Some(contract.id()), None).await?;
        call::storage_deposit(token_contract, &council, Some(dao_contract.id()), None).await?;
        call::mint_tokens(token_contract, dao_contract.id(), default_amount).await?;

        let (proposal_id, _) = call::propose_deposit_tokens(
            &council,
            dao_contract.id(),
            token_contract.id(),
            contract.id(),
            default_amount,
        )
        .await?;
        call::act_proposal(
            &council,
            dao_contract.id(),
            proposal_id,
            Action::VoteApprove,
        )
        .await?;
    }

    let deposits = view::get_deposits(&contract).await?;
    assert_eq!(
        deposits,
        token_contracts
            .iter()
            .map(|token_contract| (token_contract.id().clone(), U128(default_amount)))
            .collect::<Vec<_>>()
    );

    let shares = 1_000_000_000;

    let (proposal_id, _) =
        call::propose_mint_shares(&council, dao_contract.id(), contract.id(), shares).await?;
    call::act_proposal(
        &council,
        dao_contract.id(),
        proposal_id,
        Action::VoteApprove,
    )
    .await?;

    let total_supply = view::ft_total_supply(&contract).await?;
    assert_eq!(total_supply.0, shares);
    let balance = view::ft_balance_of(&contract, dao_contract.id()).await?;
    assert_eq!(balance.0, shares);

    let block = worker.view_block().await?;
    dbg!(block.timestamp());

    let (proposal_id, _) = call::propose_create_farm(
        &council,
        dao_contract.id(),
        contract.id(),
        pool_contract.id(),
        shares,
        "Dogshit".to_string(),
        block.timestamp() + 1_000_000_000 * 60,      // 1min
        block.timestamp() + 1_000_000_000 * 60 * 10, // 10min
    )
    .await?;
    call::act_proposal(
        &council,
        dao_contract.id(),
        proposal_id,
        Action::VoteApprove,
    )
    .await?;

    let total_supply = view::ft_total_supply(&contract).await?;
    assert_eq!(total_supply.0, shares);
    let balance = view::ft_balance_of(&contract, dao_contract.id()).await?;
    assert_eq!(balance.0, 0);
    let balance = view::ft_balance_of(&contract, pool_contract.id()).await?;
    assert_eq!(balance.0, shares);

    Ok(())
}
