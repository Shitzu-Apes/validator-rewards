use anyhow::{bail, Result};

pub struct PreviewInput {
    pub existing_basket_usd: f64,
    pub new_deposits_usd: f64,
    pub current_supply: f64,
    pub new_shares: f64,
    pub farmed_shares: f64,
    pub staked_usd: f64,
    pub duration_days: f64,
    pub dogshit_market_price_usd: Option<f64>,
    pub fee_fraction: Option<f64>,
}

pub struct PreviewOutput {
    pub projected_basket_usd: f64,
    pub projected_supply: f64,
    pub intrinsic_price_usd: f64,
    pub farmed_shares: f64,
    pub fair_mint_shares: Option<f64>,
    pub farmed_value_intrinsic_usd: f64,
    pub farmed_value_market_usd: Option<f64>,
    pub new_deposit_apr: f64,
    pub intrinsic_apr: f64,
    pub intrinsic_apy: f64,
    pub market_apr: Option<f64>,
    pub market_apy: Option<f64>,
    pub intrinsic_apr_after_fee: Option<f64>,
    pub intrinsic_apy_after_fee: Option<f64>,
}

pub fn calculate(input: PreviewInput) -> Result<PreviewOutput> {
    if !input.duration_days.is_finite() || input.duration_days <= 0.0 {
        bail!("farm duration must be positive");
    }
    if !input.staked_usd.is_finite() || input.staked_usd <= 0.0 {
        bail!("staked value must be positive");
    }
    if !input.new_shares.is_finite() || input.new_shares <= 0.0 {
        bail!("new DOGSHIT shares must be positive");
    }
    if !input.farmed_shares.is_finite() || input.farmed_shares <= 0.0 {
        bail!("farmed DOGSHIT shares must be positive");
    }

    let projected_basket_usd = input.existing_basket_usd + input.new_deposits_usd;
    let projected_supply = input.current_supply + input.new_shares;
    if !projected_supply.is_finite() || projected_supply <= 0.0 {
        bail!("projected DOGSHIT supply must be positive");
    }

    let intrinsic_price_usd = projected_basket_usd / projected_supply;
    let farmed_value_intrinsic_usd = input.farmed_shares * intrinsic_price_usd;

    // New shares for which the farmed DOGSHIT value equals the new deposit value
    // (i.e. the new-deposit APR and farmed intrinsic APR match).
    let leftover_shares = (input.farmed_shares - input.new_shares).max(0.0);
    let fair_mint_shares = if input.existing_basket_usd > 0.0 {
        let fair = (input.new_deposits_usd * input.current_supply
            - leftover_shares * (input.existing_basket_usd + input.new_deposits_usd))
            / input.existing_basket_usd;
        Some(fair.max(0.0))
    } else {
        None
    };
    let annualizer = 365.0 / input.duration_days;
    let new_deposit_apr = input.new_deposits_usd / input.staked_usd * annualizer;
    let intrinsic_apr = farmed_value_intrinsic_usd / input.staked_usd * annualizer;
    let intrinsic_apy = apy(intrinsic_apr, input.duration_days);

    let farmed_value_market_usd = input
        .dogshit_market_price_usd
        .map(|price| input.farmed_shares * price);
    let market_apr = farmed_value_market_usd.map(|value| value / input.staked_usd * annualizer);
    let market_apy = market_apr.map(|apr| apy(apr, input.duration_days));

    let fee_fraction = input.fee_fraction.map(|fee| fee.clamp(0.0, 1.0));
    let intrinsic_apr_after_fee = fee_fraction.map(|fee| intrinsic_apr * (1.0 - fee));
    let intrinsic_apy_after_fee =
        fee_fraction.map(|fee| apy(intrinsic_apr * (1.0 - fee), input.duration_days));

    Ok(PreviewOutput {
        projected_basket_usd,
        projected_supply,
        intrinsic_price_usd,
        farmed_shares: input.farmed_shares,
        fair_mint_shares,
        farmed_value_intrinsic_usd,
        farmed_value_market_usd,
        new_deposit_apr,
        intrinsic_apr,
        intrinsic_apy,
        market_apr,
        market_apy,
        intrinsic_apr_after_fee,
        intrinsic_apy_after_fee,
    })
}

fn apy(apr_fraction: f64, duration_days: f64) -> f64 {
    let periods_per_year = 365.0 / duration_days;
    let base = 1.0 + apr_fraction * duration_days / 365.0;
    if base <= 0.0 || !base.is_finite() {
        return -1.0;
    }
    base.powf(periods_per_year) - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(new_shares: f64) -> PreviewInput {
        PreviewInput {
            existing_basket_usd: 1_000.0,
            new_deposits_usd: 500.0,
            current_supply: 1_000.0,
            new_shares,
            farmed_shares: new_shares,
            staked_usd: 10_000.0,
            duration_days: 365.0,
            dogshit_market_price_usd: None,
            fee_fraction: None,
        }
    }

    #[test]
    fn proportional_mint_keeps_intrinsic_value_equal_to_deposit() {
        let output = calculate(input(500.0)).unwrap();
        assert!((output.projected_basket_usd - 1_500.0).abs() < 1e-9);
        assert!((output.intrinsic_price_usd - 1.0).abs() < 1e-9);
        assert!((output.farmed_value_intrinsic_usd - 500.0).abs() < 1e-9);
        assert!((output.new_deposit_apr - 0.05).abs() < 1e-9);
        assert!((output.intrinsic_apr - 0.05).abs() < 1e-9);
        assert!((output.fair_mint_shares.unwrap() - 500.0).abs() < 1e-9);
    }

    #[test]
    fn large_mint_dilutes_existing_holders_and_increases_staker_value() {
        let output = calculate(input(1_000.0)).unwrap();
        assert!((output.intrinsic_price_usd - 0.75).abs() < 1e-9);
        assert!((output.farmed_value_intrinsic_usd - 750.0).abs() < 1e-9);
        assert!((output.intrinsic_apr - 0.075).abs() < 1e-9);
        assert!((output.fair_mint_shares.unwrap() - 500.0).abs() < 1e-9);
    }

    #[test]
    fn includes_leftover_farm_shares_in_distributed_value() {
        let mut input = input(500.0);
        input.farmed_shares = 750.0;
        let output = calculate(input).unwrap();
        assert!((output.farmed_shares - 750.0).abs() < 1e-9);
        assert!((output.farmed_value_intrinsic_usd - 750.0).abs() < 1e-9);
        assert!((output.intrinsic_apr - 0.075).abs() < 1e-9);
        // 250 leftover shares reduce the fair new mint to 125 shares.
        assert!((output.fair_mint_shares.unwrap() - 125.0).abs() < 1e-9);
    }

    #[test]
    fn annualizes_short_durations() {
        let mut input = input(500.0);
        input.duration_days = 182.5;
        let output = calculate(input).unwrap();
        assert!((output.new_deposit_apr - 0.10).abs() < 1e-9);
        assert!((output.intrinsic_apy - 0.1025).abs() < 1e-9);
    }

    #[test]
    fn applies_market_price_and_fee() {
        let mut input = input(500.0);
        input.dogshit_market_price_usd = Some(2.0);
        input.fee_fraction = Some(0.25);
        let output = calculate(input).unwrap();
        assert!((output.farmed_value_market_usd.unwrap() - 1_000.0).abs() < 1e-9);
        assert!((output.market_apr.unwrap() - 0.10).abs() < 1e-9);
        assert!((output.intrinsic_apr_after_fee.unwrap() - 0.0375).abs() < 1e-9);
    }

    #[test]
    fn rejects_invalid_inputs() {
        let mut invalid = input(500.0);
        invalid.staked_usd = 0.0;
        assert!(calculate(invalid).is_err());

        let mut invalid = input(0.0);
        invalid.duration_days = 0.0;
        assert!(calculate(invalid).is_err());
    }
}
