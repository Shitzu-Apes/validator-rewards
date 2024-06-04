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
            nft_contract,
            rewarder_contract,
            token_contracts,
            ..
        } = initialize_contracts().await?;

        let mint_amount = 1_000_000;

        call::nft_mint(&council, nft_contract.id(), 1).await?;
        let [token] = &view::nft_tokens_for_owner(&nft_contract, council.id()).await?[..] else {
            return Err(anyhow::anyhow!("No NFT tokens"));
        };
        call::stake_nft_with_rewarder(
            &council,
            nft_contract.id(),
            rewarder_contract.id(),
            &token.token_id,
        )
        .await?;
        assert!(view::primary_nft_of(&rewarder_contract, council.id())
            .await?
            .is_some());

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
        let end_date = block.timestamp() + 1_000_000_000 * 60 * 5; // 5min
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
        let farm = view::get_farm(&pool_contract, 0).await?;
        assert!(farm.active);
        assert!(farm.start_date.0 > worker.view_block().await?.timestamp());

        call::deposit_and_stake(&council, pool_contract.id(), NearToken::from_near(10_000)).await?;
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
        let total_supply = view::ft_total_supply(&contract).await?;
        let burnt_shares = shares - total_supply.0;
        for (token_id, amount) in rewards {
            let distributed = (burnt_shares * mint_amount) / shares;
            assert_eq!(amount.0, mint_amount - distributed);
            for token_contract in &token_contracts {
                let balance = view::ft_balance_of(token_contract, council.id()).await?;
                assert_eq!(balance.0, distributed);
                if &token_id == token_contracts[0].id() {
                    let (_, score) = view::primary_nft_of(&rewarder_contract, council.id())
                        .await?
                        .unwrap();
                    assert_eq!(score.0, balance.0 * 3);
                }
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
        for (token_id, amount) in rewards {
            let distributed = (burnt_shares * mint_amount) / shares;
            assert!(amount.0 < mint_amount / 100);
            for token_contract in &token_contracts {
                let balance = view::ft_balance_of(token_contract, council.id()).await?;
                assert_eq!(balance.0, distributed);
                if &token_id == token_contracts[0].id() {
                    let (_, score) = view::primary_nft_of(&rewarder_contract, council.id())
                        .await?
                        .unwrap();
                    assert_eq!(score.0, balance.0 * 3);
                }
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

#[tokio::test]
async fn test_multiple_users_reward_distribution() -> anyhow::Result<()> {
    let mut chain = initialize_blockchain().await?;

    let thread = tokio::spawn(async {
        let Init {
            worker,
            near,
            council,
            contract,
            dao_contract,
            pool_contract,
            nft_contract,
            rewarder_contract,
            token_contracts,
            ..
        } = initialize_contracts().await?;

        let user_a = near
            .create_subaccount("a")
            .initial_balance(NearToken::from_near(100_000))
            .transact()
            .await?
            .into_result()?;
        let user_b = near
            .create_subaccount("b")
            .initial_balance(NearToken::from_near(100_000))
            .transact()
            .await?
            .into_result()?;

        call::nft_mint(&user_a, nft_contract.id(), 1).await?;
        let [token] = &view::nft_tokens_for_owner(&nft_contract, user_a.id()).await?[..] else {
            return Err(anyhow::anyhow!("No NFT tokens"));
        };
        call::stake_nft_with_rewarder(
            &user_a,
            nft_contract.id(),
            rewarder_contract.id(),
            &token.token_id,
        )
        .await?;
        assert!(view::primary_nft_of(&rewarder_contract, user_a.id())
            .await?
            .is_some());
        call::nft_mint(&user_b, nft_contract.id(), 1).await?;
        let [token] = &view::nft_tokens_for_owner(&nft_contract, user_b.id()).await?[..] else {
            return Err(anyhow::anyhow!("No NFT tokens"));
        };
        call::stake_nft_with_rewarder(
            &user_b,
            nft_contract.id(),
            rewarder_contract.id(),
            &token.token_id,
        )
        .await?;
        assert!(view::primary_nft_of(&rewarder_contract, user_b.id())
            .await?
            .is_some());

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
            let user_a = user_a.clone();
            let user_b = user_b.clone();
            let contract = contract.clone();
            let dao_contract = dao_contract.clone();
            tokio::spawn(async move {
                call::storage_deposit(&token_contract, &council, None, None).await?;
                call::storage_deposit(&token_contract, &council, Some(contract.id()), None).await?;
                call::storage_deposit(&token_contract, &council, Some(dao_contract.id()), None)
                    .await?;
                call::storage_deposit(&token_contract, &council, Some(user_a.id()), None).await?;
                call::storage_deposit(&token_contract, &council, Some(user_b.id()), None).await?;
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
        deposits.sort_by_key(|deposit: &(near_sdk::AccountId, U128)| deposit.0.clone());

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
        let end_date = block.timestamp() + 1_000_000_000 * 60 * 5; // 5min
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
        let farm = view::get_farm(&pool_contract, 0).await?;
        assert!(farm.active);
        assert!(farm.start_date.0 > worker.view_block().await?.timestamp());

        call::deposit_and_stake(&user_a, pool_contract.id(), NearToken::from_near(10_000)).await?;
        let account = view::get_account(&pool_contract, user_a.id()).await?;
        assert_eq!(
            account.staked_balance.0,
            NearToken::from_near(10_000).as_yoctonear()
        );

        call::deposit_and_stake(&user_b, pool_contract.id(), NearToken::from_near(1_000)).await?;
        let account = view::get_account(&pool_contract, user_b.id()).await?;
        assert_eq!(
            account.staked_balance.0,
            NearToken::from_near(1_000).as_yoctonear()
        );

        while worker.view_block().await?.timestamp() < start_date {
            worker.fast_forward(5).await?;
        }

        let farm = view::get_farm(&pool_contract, 0).await?;
        assert!(farm.active);
        assert!(farm.start_date.0 < worker.view_block().await?.timestamp());
        worker.fast_forward(5).await?;

        call::claim(&user_a, pool_contract.id(), contract.id()).await?;
        let unclaimed_a = view::get_unclaimed_reward(&pool_contract, user_a.id(), 0).await?;
        assert!(unclaimed_a.0 > 0);
        assert!(unclaimed_a.0 < shares);
        let balance_a = view::ft_balance_of(&contract, user_a.id()).await?;
        assert!(balance_a.0 > 0);
        assert!(balance_a.0 < shares);
        let balance = view::ft_balance_of(&contract, pool_contract.id()).await?;
        assert!(balance.0 > 0);
        assert!(balance.0 < shares);

        call::claim(&user_b, pool_contract.id(), contract.id()).await?;
        let unclaimed_b = view::get_unclaimed_reward(&pool_contract, user_b.id(), 0).await?;
        assert!(unclaimed_b.0 > 0);
        assert!(unclaimed_b.0 < shares);
        let balance_b = view::ft_balance_of(&contract, user_b.id()).await?;
        assert!(balance_b.0 > 0);
        assert!(balance_b.0 < shares);
        let balance = view::ft_balance_of(&contract, pool_contract.id()).await?;
        assert!(balance.0 > 0);
        assert!(balance.0 < shares);

        assert!(balance_a.0 > balance_b.0);
        assert!(balance_a.0 < balance_b.0 * 10);
        assert!(balance_a.0 > balance_b.0);
        assert!(balance_a.0 < balance_b.0 * 10);

        let (burnt_shares, _) = call::burn(&user_a, contract.id()).await?;
        assert!(burnt_shares.0 > 0);
        let (burnt_shares, _) = call::burn(&user_b, contract.id()).await?;
        assert!(burnt_shares.0 > 0);

        let rewards = view::get_undistributed_rewards(&contract).await?;
        let total_supply = view::ft_total_supply(&contract).await?;
        let burnt_shares = shares - total_supply.0;
        for (_, amount) in rewards {
            let distributed = (burnt_shares * mint_amount) / shares;
            assert_eq!(amount.0, mint_amount - distributed);
            for token_contract in &token_contracts {
                let balance_a = view::ft_balance_of(token_contract, user_a.id()).await?;
                let balance_b = view::ft_balance_of(token_contract, user_b.id()).await?;
                assert_eq!(balance_a.0 + balance_b.0, distributed);
            }
        }

        while worker.view_block().await?.timestamp() < end_date {
            worker.fast_forward(5).await?;
        }

        call::claim(&user_a, pool_contract.id(), contract.id()).await?;
        let unclaimed = view::get_unclaimed_reward(&pool_contract, user_a.id(), 0).await?;
        assert!(unclaimed.0 < shares / 100);
        let balance = view::ft_balance_of(&contract, user_a.id()).await?;
        assert!(balance.0 > 0);
        assert!(balance.0 < shares);

        call::claim(&user_b, pool_contract.id(), contract.id()).await?;
        let unclaimed = view::get_unclaimed_reward(&pool_contract, user_b.id(), 0).await?;
        assert!(unclaimed.0 < shares / 100);
        let balance = view::ft_balance_of(&contract, user_b.id()).await?;
        assert!(balance.0 > 0);
        assert!(balance.0 < shares);

        let balance = view::ft_balance_of(&contract, pool_contract.id()).await?;
        assert!(balance.0 < shares / 100);

        let (burnt_shares, _) = call::burn(&user_a, contract.id()).await?;
        assert!(burnt_shares.0 > 0);
        let (burnt_shares, _) = call::burn(&user_b, contract.id()).await?;
        assert!(burnt_shares.0 > 0);

        let rewards = view::get_undistributed_rewards(&contract).await?;
        let total_supply = view::ft_total_supply(&contract).await?;
        let burnt_shares = shares - total_supply.0;
        for (_, amount) in rewards {
            let distributed = (burnt_shares * mint_amount) / shares;
            assert!(amount.0 < mint_amount / 100);
            for token_contract in &token_contracts {
                let balance_a = view::ft_balance_of(token_contract, user_a.id()).await?;
                let balance_b = view::ft_balance_of(token_contract, user_b.id()).await?;
                assert_eq!(balance_a.0 + balance_b.0, distributed);
            }
        }
        let balance = view::ft_balance_of(&contract, user_a.id()).await?;
        assert!(balance.0 == 0);
        let balance = view::ft_balance_of(&contract, user_b.id()).await?;
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

#[tokio::test]
async fn test_refresh_rewards_when_active() -> anyhow::Result<()> {
    let mut chain = initialize_blockchain().await?;

    let thread =
        tokio::spawn(async {
            let Init {
                worker,
                council,
                contract,
                dao_contract,
                pool_contract,
                nft_contract,
                rewarder_contract,
                token_contracts,
                ..
            } = initialize_contracts().await?;

            call::nft_mint(&council, nft_contract.id(), 1).await?;
            let [token] = &view::nft_tokens_for_owner(&nft_contract, council.id()).await?[..]
            else {
                return Err(anyhow::anyhow!("No NFT tokens"));
            };
            call::stake_nft_with_rewarder(
                &council,
                nft_contract.id(),
                rewarder_contract.id(),
                &token.token_id,
            )
            .await?;
            assert!(view::primary_nft_of(&rewarder_contract, council.id())
                .await?
                .is_some());

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

            try_join_all(token_contracts.iter().cloned().enumerate().map(
                |(index, token_contract)| {
                    let council = council.clone();
                    let contract = contract.clone();
                    let dao_contract = dao_contract.clone();
                    tokio::spawn(async move {
                        call::storage_deposit(&token_contract, &council, None, None).await?;
                        call::storage_deposit(&token_contract, &council, Some(contract.id()), None)
                            .await?;
                        call::storage_deposit(
                            &token_contract,
                            &council,
                            Some(dao_contract.id()),
                            None,
                        )
                        .await?;
                        call::mint_tokens(&token_contract, dao_contract.id(), mint_amount).await?;

                        if index == 2 {
                            return anyhow::Ok(());
                        }
                        let mint_amount = if index == 0 {
                            mint_amount
                        } else {
                            mint_amount / 2
                        };
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
                },
            ))
            .await?;

            // Dogshit has same amount of decimals as NEAR
            // WARNING: the staking-farm contract doesn't work, if too few tokens are added for distribution
            let shares = NearToken::from_near(1).as_yoctonear();

            let (proposal_id, _) =
                call::propose_mint_shares(&council, dao_contract.id(), contract.id(), shares)
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
            assert_eq!(balance.0, shares);

            let block = worker.view_block().await?;

            let start_date = block.timestamp() + 1_000_000_000 * 60; // 1min
            let end_date = block.timestamp() + 1_000_000_000 * 60 * 5; // 5min
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
            let farm = view::get_farm(&pool_contract, 0).await?;
            assert!(farm.active);
            assert!(farm.start_date.0 > worker.view_block().await?.timestamp());

            call::deposit_and_stake(&council, pool_contract.id(), NearToken::from_near(10_000))
                .await?;
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
            let balance_0 = view::ft_balance_of(&token_contracts[0], council.id()).await?;
            let balance_1 = view::ft_balance_of(&token_contracts[1], council.id()).await?;
            let balance_2 = view::ft_balance_of(&token_contracts[2], council.id()).await?;
            assert_eq!(balance_2.0, 0);
            assert_eq!(balance_0.0 / 2, balance_1.0);

            // mint more shares with different tokens
            try_join_all(token_contracts.iter().cloned().enumerate().map(
                |(index, token_contract)| {
                    let council = council.clone();
                    let contract = contract.clone();
                    let dao_contract = dao_contract.clone();
                    tokio::spawn(async move {
                        if index == 0 {
                            return anyhow::Ok(());
                        }
                        let mint_amount = if index == 2 {
                            mint_amount
                        } else {
                            mint_amount / 2
                        };
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
                },
            ))
            .await?;

            let (proposal_id, _) =
                call::propose_mint_shares(&council, dao_contract.id(), contract.id(), shares)
                    .await?;
            call::act_proposal(
                &council,
                dao_contract.id(),
                proposal_id,
                Action::VoteApprove,
            )
            .await?;

            let old_farm = farm;
            let block = worker.view_block().await?;
            let end_date = block.timestamp() + 1_000_000_000 * 60 * 5; // 5min
            let (proposal_id, _) = call::propose_update_farm(
                &council,
                dao_contract.id(),
                contract.id(),
                pool_contract.id(),
                shares,
                "Dogshit".to_string(),
                end_date,
                0,
            )
            .await?;
            call::act_proposal(
                &council,
                dao_contract.id(),
                proposal_id,
                Action::VoteApprove,
            )
            .await?;

            let farm = view::get_farm(&pool_contract, 0).await?;
            assert!(farm.active);
            assert!(farm.start_date.0 > old_farm.start_date.0);
            assert!(farm.end_date.0 > old_farm.end_date.0);
            assert!(farm.amount.0 > shares);
            assert!(farm.amount.0 < 2 * shares);

            call::claim(&council, pool_contract.id(), contract.id()).await?;

            let (burnt_shares, _) = call::burn(&council, contract.id()).await?;
            assert!(burnt_shares.0 > 0);
            let balance_0 = view::ft_balance_of(&token_contracts[0], council.id()).await?;
            let balance_1 = view::ft_balance_of(&token_contracts[1], council.id()).await?;
            let balance_2 = view::ft_balance_of(&token_contracts[2], council.id()).await?;
            assert!(balance_2.0 > 0);
            assert!(balance_0.0 > balance_1.0);
            assert!(balance_1.0 > balance_2.0);

            while worker.view_block().await?.timestamp() < end_date {
                worker.fast_forward(5).await?;
            }

            call::claim(&council, pool_contract.id(), contract.id()).await?;

            let (burnt_shares, _) = call::burn(&council, contract.id()).await?;
            assert!(burnt_shares.0 > 0);
            let balance_0 = view::ft_balance_of(&token_contracts[0], council.id()).await?;
            let balance_1 = view::ft_balance_of(&token_contracts[1], council.id()).await?;
            let balance_2 = view::ft_balance_of(&token_contracts[2], council.id()).await?;
            assert!(balance_0.0 > mint_amount * 99 / 100);
            assert!(balance_1.0 > mint_amount * 99 / 100);
            assert!(balance_2.0 > mint_amount * 99 / 100);

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

#[tokio::test]
async fn test_refresh_rewards_when_inactive() -> anyhow::Result<()> {
    let mut chain = initialize_blockchain().await?;

    let thread =
        tokio::spawn(async {
            let Init {
                worker,
                council,
                contract,
                dao_contract,
                pool_contract,
                nft_contract,
                rewarder_contract,
                token_contracts,
                ..
            } = initialize_contracts().await?;

            call::nft_mint(&council, nft_contract.id(), 1).await?;
            let [token] = &view::nft_tokens_for_owner(&nft_contract, council.id()).await?[..]
            else {
                return Err(anyhow::anyhow!("No NFT tokens"));
            };
            call::stake_nft_with_rewarder(
                &council,
                nft_contract.id(),
                rewarder_contract.id(),
                &token.token_id,
            )
            .await?;
            assert!(view::primary_nft_of(&rewarder_contract, council.id())
                .await?
                .is_some());

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

            try_join_all(token_contracts.iter().cloned().enumerate().map(
                |(index, token_contract)| {
                    let council = council.clone();
                    let contract = contract.clone();
                    let dao_contract = dao_contract.clone();
                    tokio::spawn(async move {
                        call::storage_deposit(&token_contract, &council, None, None).await?;
                        call::storage_deposit(&token_contract, &council, Some(contract.id()), None)
                            .await?;
                        call::storage_deposit(
                            &token_contract,
                            &council,
                            Some(dao_contract.id()),
                            None,
                        )
                        .await?;
                        call::mint_tokens(&token_contract, dao_contract.id(), mint_amount).await?;

                        if index == 2 {
                            return anyhow::Ok(());
                        }
                        let mint_amount = if index == 0 {
                            mint_amount
                        } else {
                            mint_amount / 2
                        };
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
                },
            ))
            .await?;

            // Dogshit has same amount of decimals as NEAR
            // WARNING: the staking-farm contract doesn't work, if too few tokens are added for distribution
            let shares = NearToken::from_near(1).as_yoctonear();

            let (proposal_id, _) =
                call::propose_mint_shares(&council, dao_contract.id(), contract.id(), shares)
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
            assert_eq!(balance.0, shares);

            let block = worker.view_block().await?;

            let start_date = block.timestamp() + 1_000_000_000 * 60; // 1min
            let end_date = block.timestamp() + 1_000_000_000 * 60 * 5; // 5min
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
            let farm = view::get_farm(&pool_contract, 0).await?;
            assert!(farm.active);
            assert!(farm.start_date.0 > worker.view_block().await?.timestamp());

            call::deposit_and_stake(&council, pool_contract.id(), NearToken::from_near(10_000))
                .await?;
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
            let balance_0 = view::ft_balance_of(&token_contracts[0], council.id()).await?;
            let balance_1 = view::ft_balance_of(&token_contracts[1], council.id()).await?;
            let balance_2 = view::ft_balance_of(&token_contracts[2], council.id()).await?;
            assert_eq!(balance_2.0, 0);
            assert_eq!(balance_0.0 / 2, balance_1.0);

            // mint more shares with different tokens
            try_join_all(token_contracts.iter().cloned().enumerate().map(
                |(index, token_contract)| {
                    let council = council.clone();
                    let contract = contract.clone();
                    let dao_contract = dao_contract.clone();
                    tokio::spawn(async move {
                        if index == 0 {
                            return anyhow::Ok(());
                        }
                        let mint_amount = if index == 2 {
                            mint_amount
                        } else {
                            mint_amount / 2
                        };
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
                },
            ))
            .await?;

            let (proposal_id, _) =
                call::propose_mint_shares(&council, dao_contract.id(), contract.id(), shares)
                    .await?;
            call::act_proposal(
                &council,
                dao_contract.id(),
                proposal_id,
                Action::VoteApprove,
            )
            .await?;

            while worker.view_block().await?.timestamp() < end_date {
                worker.fast_forward(5).await?;
            }
            call::claim(&council, pool_contract.id(), contract.id()).await?;
            let old_farm = view::get_farm(&pool_contract, 0).await?;

            let block = worker.view_block().await?;
            let end_date = block.timestamp() + 1_000_000_000 * 60 * 5; // 5min
            let (proposal_id, _) = call::propose_update_farm(
                &council,
                dao_contract.id(),
                contract.id(),
                pool_contract.id(),
                shares,
                "Dogshit".to_string(),
                end_date,
                0,
            )
            .await?;
            call::act_proposal(
                &council,
                dao_contract.id(),
                proposal_id,
                Action::VoteApprove,
            )
            .await?;

            let farm = view::get_farm(&pool_contract, 0).await?;
            assert!(farm.active);
            assert!(farm.start_date.0 > old_farm.start_date.0);
            assert!(farm.end_date.0 > old_farm.end_date.0);
            assert_eq!(farm.amount.0, shares);

            while worker.view_block().await?.timestamp() < end_date {
                worker.fast_forward(5).await?;
            }

            call::claim(&council, pool_contract.id(), contract.id()).await?;

            let (burnt_shares, _) = call::burn(&council, contract.id()).await?;
            assert!(burnt_shares.0 > 0);
            let balance_0 = view::ft_balance_of(&token_contracts[0], council.id()).await?;
            let balance_1 = view::ft_balance_of(&token_contracts[1], council.id()).await?;
            let balance_2 = view::ft_balance_of(&token_contracts[2], council.id()).await?;
            assert!(balance_0.0 > mint_amount * 99 / 100);
            assert!(balance_1.0 > mint_amount * 99 / 100);
            assert!(balance_2.0 > mint_amount * 99 / 100);

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

#[tokio::test]
async fn test_no_nft_penalty() -> anyhow::Result<()> {
    let mut chain = initialize_blockchain().await?;

    let thread = tokio::spawn(async {
        let Init {
            worker,
            council,
            contract,
            dao_contract,
            pool_contract,
            token_contracts,
            ..
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
        let end_date = block.timestamp() + 1_000_000_000 * 60 * 5; // 5min
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
        let farm = view::get_farm(&pool_contract, 0).await?;
        assert!(farm.active);
        assert!(farm.start_date.0 > worker.view_block().await?.timestamp());

        call::deposit_and_stake(&council, pool_contract.id(), NearToken::from_near(10_000)).await?;
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
        let total_supply = view::ft_total_supply(&contract).await?;
        let burnt_shares = shares - total_supply.0;
        for (_, amount) in rewards {
            let distributed = (burnt_shares * mint_amount) / shares;
            assert_eq!(amount.0, mint_amount - distributed);
            for token_contract in &token_contracts {
                let balance = view::ft_balance_of(token_contract, council.id()).await?;
                assert_eq!(balance.0, distributed);
            }
        }
        let balance = view::ft_balance_of(&contract, dao_contract.id()).await?;
        assert_eq!(balance.0 * 5, burnt_shares + balance.0);

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
            let distributed = (burnt_shares * mint_amount) / shares;
            assert!(amount.0 < mint_amount * 21 / 100);
            for token_contract in &token_contracts {
                let balance = view::ft_balance_of(token_contract, council.id()).await?;
                assert_eq!(balance.0, distributed);
            }
        }
        let balance = view::ft_balance_of(&contract, council.id()).await?;
        assert!(balance.0 == 0);

        let (proposal_id, _) =
            call::propose_burn(&council, dao_contract.id(), contract.id()).await?;
        call::act_proposal(
            &council,
            dao_contract.id(),
            proposal_id,
            Action::VoteApprove,
        )
        .await?;

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
