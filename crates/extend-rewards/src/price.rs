use anyhow::{bail, Context, Result};
use near_sdk::AccountId;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

const REF_PRICES_URL: &str = "https://api.ref.finance/list-token-price";
const DEXSCREENER_URL: &str = "https://api.dexscreener.com/latest/dex/tokens";

#[derive(Deserialize)]
struct RefPrice {
    price: String,
    symbol: Option<String>,
    decimal: Option<u8>,
}

#[derive(Clone, Debug)]
pub struct RefTokenInfo {
    pub usd: f64,
    pub symbol: Option<String>,
    pub decimals: Option<u8>,
}

#[derive(Deserialize)]
struct DexResponse {
    pairs: Option<Vec<DexPair>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DexPair {
    base_token: DexToken,
    quote_token: DexToken,
    price_usd: Option<String>,
    price_native: Option<String>,
    liquidity: Option<DexLiquidity>,
}

#[derive(Deserialize)]
struct DexToken {
    address: String,
}

#[derive(Deserialize)]
struct DexLiquidity {
    usd: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct TokenPrice {
    pub usd: f64,
    pub source: &'static str,
}

#[derive(Default)]
pub struct PriceOverrides {
    entries: Vec<(String, f64)>,
}

impl PriceOverrides {
    pub fn parse(values: &[String]) -> Result<Self> {
        let mut entries = Vec::new();
        for value in values {
            let (key, price) = value
                .split_once('=')
                .with_context(|| format!("expected TOKEN=USD, got `{value}`"))?;
            let key = key.trim().to_ascii_lowercase();
            if key.is_empty() {
                bail!("token is empty in `{value}`");
            }
            let price: f64 = price
                .trim()
                .parse()
                .with_context(|| format!("invalid USD price in `{value}`"))?;
            if !price.is_finite() || price < 0.0 {
                bail!("price must be a finite non-negative number in `{value}`");
            }
            entries.push((key, price));
        }
        Ok(Self { entries })
    }

    pub fn get(&self, token: &AccountId, symbol: &str) -> Option<f64> {
        let token = token.as_str().to_ascii_lowercase();
        let symbol = symbol.to_ascii_lowercase();
        self.entries
            .iter()
            .rev()
            .find(|(key, _)| key == &token || key == &symbol)
            .map(|(_, price)| *price)
    }
}

pub fn build_client() -> Result<Client> {
    Client::builder()
        .user_agent(concat!("extend-rewards/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .context("failed to build HTTP client")
}

/// Fetches the Ref Finance price/metadata list once. Keys are token account IDs.
pub async fn fetch_ref_prices(client: &Client) -> Result<HashMap<String, RefTokenInfo>> {
    let response = client
        .get(REF_PRICES_URL)
        .send()
        .await
        .context("failed to fetch Ref Finance prices")?
        .error_for_status()
        .context("Ref Finance prices returned an error status")?;
    let prices: HashMap<String, RefPrice> = response
        .json()
        .await
        .context("failed to parse Ref Finance prices")?;
    Ok(prices
        .into_iter()
        .filter_map(|(token, entry)| {
            let usd = entry
                .price
                .parse::<f64>()
                .ok()
                .filter(|price| price.is_finite() && *price > 0.0)?;
            Some((
                token,
                RefTokenInfo {
                    usd,
                    symbol: entry.symbol,
                    decimals: entry.decimal,
                },
            ))
        })
        .collect())
}

pub async fn fetch_dexscreener_price(client: &Client, token: &AccountId) -> Result<Option<f64>> {
    let response = client
        .get(format!("{DEXSCREENER_URL}/{token}"))
        .send()
        .await
        .context("failed to fetch DexScreener prices")?
        .error_for_status()
        .context("DexScreener returned an error status")?;
    let response: DexResponse = response
        .json()
        .await
        .context("failed to parse DexScreener prices")?;
    Ok(select_dexscreener_price(
        response.pairs.as_deref().unwrap_or_default(),
        token.as_str(),
    ))
}

fn select_dexscreener_price(pairs: &[DexPair], token: &str) -> Option<f64> {
    let mut best: Option<(f64, f64)> = None;
    for pair in pairs {
        let Some(price_usd) = pair
            .price_usd
            .as_deref()
            .and_then(|p| p.parse::<f64>().ok())
        else {
            continue;
        };
        if !price_usd.is_finite() || price_usd <= 0.0 {
            continue;
        }

        let price = if pair.base_token.address.eq_ignore_ascii_case(token) {
            price_usd
        } else if pair.quote_token.address.eq_ignore_ascii_case(token) {
            let Some(price_native) = pair
                .price_native
                .as_deref()
                .and_then(|p| p.parse::<f64>().ok())
            else {
                continue;
            };
            if !price_native.is_finite() || price_native <= 0.0 {
                continue;
            }
            price_usd / price_native
        } else {
            continue;
        };

        let liquidity = pair
            .liquidity
            .as_ref()
            .and_then(|liquidity| liquidity.usd)
            .unwrap_or(0.0);
        if best.is_none_or(|(_, best_liquidity)| liquidity > best_liquidity) {
            best = Some((price, liquidity));
        }
    }
    best.map(|(price, _)| price)
}

/// Resolves a token price: explicit override, then Ref Finance, then DexScreener.
pub async fn resolve_price(
    client: &Client,
    ref_prices: &HashMap<String, RefTokenInfo>,
    overrides: &PriceOverrides,
    token: &AccountId,
    symbol: &str,
) -> Result<Option<TokenPrice>> {
    if let Some(usd) = overrides.get(token, symbol) {
        return Ok(Some(TokenPrice {
            usd,
            source: "override",
        }));
    }
    if let Some(info) = ref_prices.get(token.as_str()) {
        return Ok(Some(TokenPrice {
            usd: info.usd,
            source: "ref-finance",
        }));
    }
    Ok(fetch_dexscreener_price(client, token)
        .await?
        .map(|usd| TokenPrice {
            usd,
            source: "dexscreener",
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(
        base: &str,
        quote: &str,
        price_usd: &str,
        price_native: &str,
        liquidity: f64,
    ) -> DexPair {
        DexPair {
            base_token: DexToken {
                address: base.to_string(),
            },
            quote_token: DexToken {
                address: quote.to_string(),
            },
            price_usd: Some(price_usd.to_string()),
            price_native: Some(price_native.to_string()),
            liquidity: Some(DexLiquidity {
                usd: Some(liquidity),
            }),
        }
    }

    #[test]
    fn selects_highest_liquidity_base_pair() {
        let pairs = vec![
            pair("token.near", "wrap.near", "0.01", "0.003", 100.0),
            pair("token.near", "usdc.near", "0.02", "0.006", 900.0),
            pair("other.near", "token.near", "10.0", "500.0", 5000.0),
        ];
        assert_eq!(select_dexscreener_price(&pairs, "token.near"), Some(0.02));
    }

    #[test]
    fn computes_price_when_token_is_quote() {
        let pairs = vec![pair("wrap.near", "token.near", "3.50", "1000.0", 42.0)];
        assert_eq!(select_dexscreener_price(&pairs, "token.near"), Some(0.0035));
    }

    #[test]
    fn parses_price_overrides_by_symbol_or_account() {
        let overrides = PriceOverrides::parse(&[
            "SHITZU=0.0015".to_string(),
            "token.0xshitzu.near=0.002".to_string(),
        ])
        .unwrap();
        let token: AccountId = "token.0xshitzu.near".parse().unwrap();
        assert_eq!(overrides.get(&token, "SHITZU"), Some(0.002));
        let other: AccountId = "other.near".parse().unwrap();
        assert_eq!(overrides.get(&other, "SHITZU"), Some(0.0015));
        assert_eq!(overrides.get(&other, "LONK"), None);
    }

    #[test]
    fn rejects_bad_prices() {
        assert!(PriceOverrides::parse(&["SHITZU=-1".to_string()]).is_err());
        assert!(PriceOverrides::parse(&["SHITZU".to_string()]).is_err());
        assert!(PriceOverrides::parse(&["SHITZU=nope".to_string()]).is_err());
    }
}
