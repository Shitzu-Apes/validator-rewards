mod util;

use futures::future::try_join_all;
use near_sdk::{json_types::U128, NearToken};
use util::*;

#[tokio::test]
async fn test_basic_reward_distribution() -> anyhow::Result<()> {
    let mut chain = initialize_blockchain().await?;

    let thread = tokio::spawn(async {
        let Init {
            worker,
            council,
            contract,
            dao_contract,
            pool_contract,
            token_contracts,
        } = initialize_contracts().await?;

        let mint_amount = 1_000_000;

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

        try_join_all(token_contracts.iter().cloned().map(|token_contract| {
            let council = council.clone();
            let contract = contract.clone();
            let dao_contract = dao_contract.clone();
            tokio::spawn(async move {
                call::storage_deposit(&token_contract, &council, None, None).await?;
                call::storage_deposit(&token_contract, &council, Some(contract.id()), None).await?;
                call::storage_deposit(&token_contract, &council, Some(dao_contract.id()), None)
                    .await?;
                call::mint_tokens(&token_contract, dao_contract.id(), mint_amount).await?;

                let (proposal_id, _) = call::propose_deposit_tokens(
                    &council,
                    dao_contract.id(),
                    token_contract.id(),
                    contract.id(),
                    mint_amount,
                )
                .await?;
                call::act_proposal(
                    &council,
                    dao_contract.id(),
                    proposal_id,
                    Action::VoteApprove,
                )
                .await?;
                anyhow::Ok(())
            })
        }))
        .await?;

        let mut deposits = view::get_deposits(&contract).await?;
        deposits.sort_by_key(|deposit| deposit.0.clone());

        assert_eq!(
            deposits,
            token_contracts
                .iter()
                .map(|token_contract| (token_contract.id().clone(), U128(mint_amount)))
                .collect::<Vec<_>>()
        );

        // Dogshit has same amount of decimals as NEAR
        // WARNING: the staking-farm contract doesn't work, if too few tokens are added for distribution
        let shares = NearToken::from_near(1).as_yoctonear();

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

        let start_date = block.timestamp() + 1_000_000_000 * 60; // 1min
        let end_date = block.timestamp() + 1_000_000_000 * 60 * 2; // 2min
        let (proposal_id, _) = call::propose_create_farm(
            &council,
            dao_contract.id(),
            contract.id(),
            pool_contract.id(),
            shares,
            "Dogshit".to_string(),
            start_date,
            end_date,
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

        call::deposit_and_stake(&council, pool_contract.id(), NearToken::from_near(10_000)).await?;
        let farm = view::get_farm(&pool_contract, 0).await?;
        assert!(farm.active);
        assert!(farm.start_date.0 > worker.view_block().await?.timestamp());
        let account = view::get_account(&pool_contract, council.id()).await?;
        assert_eq!(
            account.staked_balance.0,
            NearToken::from_near(10_000).as_yoctonear()
        );

        while worker.view_block().await?.timestamp() < start_date {
            worker.fast_forward(5).await?;
        }

        let farm = view::get_farm(&pool_contract, 0).await?;
        assert!(farm.active);
        assert!(farm.start_date.0 < worker.view_block().await?.timestamp());
        worker.fast_forward(5).await?;

        call::claim(&council, pool_contract.id(), contract.id()).await?;
        let unclaimed = view::get_unclaimed_reward(&pool_contract, council.id(), 0).await?;
        assert!(unclaimed.0 > 0);
        assert!(unclaimed.0 < shares);
        let balance = view::ft_balance_of(&contract, council.id()).await?;
        assert!(balance.0 > 0);
        assert!(balance.0 < shares);
        let balance = view::ft_balance_of(&contract, pool_contract.id()).await?;
        assert!(balance.0 > 0);
        assert!(balance.0 < shares);

        let (burnt_shares, _) = call::burn(&council, contract.id()).await?;
        assert!(burnt_shares.0 > 0);
        let rewards = view::get_undistributed_rewards(&contract).await?;
        for (_, amount) in rewards {
            let distributed = burnt_shares.0 * mint_amount / shares;
            assert_eq!(amount.0, mint_amount - distributed);
            for token_contract in &token_contracts {
                let balance = view::ft_balance_of(token_contract, council.id()).await?;
                assert_eq!(balance.0, distributed);
            }
        }

        while worker.view_block().await?.timestamp() < end_date {
            worker.fast_forward(5).await?;
        }

        call::claim(&council, pool_contract.id(), contract.id()).await?;
        let unclaimed = view::get_unclaimed_reward(&pool_contract, council.id(), 0).await?;
        assert!(unclaimed.0 < shares / 100);
        let balance = view::ft_balance_of(&contract, council.id()).await?;
        assert!(balance.0 > 0);
        assert!(balance.0 < shares);
        let balance = view::ft_balance_of(&contract, pool_contract.id()).await?;
        assert!(balance.0 < shares / 100);

        let (burnt_shares, _) = call::burn(&council, contract.id()).await?;
        assert!(burnt_shares.0 > 0);
        let rewards = view::get_undistributed_rewards(&contract).await?;
        let total_supply = view::ft_total_supply(&contract).await?;
        let burnt_shares = shares - total_supply.0;
        for (_, amount) in rewards {
            let distributed = burnt_shares * mint_amount / shares;
            assert!(amount.0 < mint_amount / 100);
            for token_contract in &token_contracts {
                let balance = view::ft_balance_of(token_contract, council.id()).await?;
                assert_eq!(balance.0, distributed);
            }
        }
        let balance = view::ft_balance_of(&contract, council.id()).await?;
        assert!(balance.0 == 0);

        anyhow::Ok(())
    })
    .await;
    chain.kill()?;
    match thread {
        Err(err) => Err(anyhow::anyhow!(err)),
        Ok(Err(err)) => Err(anyhow::anyhow!(err)),
        Ok(_) => anyhow::Ok(()),
    }
}
