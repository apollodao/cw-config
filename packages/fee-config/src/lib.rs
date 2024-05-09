use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Coins, CosmosMsg, Decimal, Deps, Env, StdError, StdResult};
use cw_address_like::AddressLike;
use cw_asset::{Asset, AssetList};

#[cw_serde]
#[derive(Default)]
/// A struct that contains a fee configuration (fee rate and recipients).
pub struct FeeConfig<T: AddressLike> {
    /// The fraction of the tokens that are taken as a fee.
    pub fee_rate: Decimal,
    /// The addresses of the recipients of the fee. Each address in the vec is
    /// paired with a Decimal, which represents the percentage of the fee
    /// that should be sent to that address. The sum of all decimals must be
    /// 1.
    pub fee_recipients: Vec<(T, Decimal)>,
}

impl FeeConfig<String> {
    /// Validates the fee config and returns a `FeeConfig<Addr>`.
    pub fn check(&self, deps: &Deps) -> StdResult<FeeConfig<Addr>> {
        // Fee rate must be between 0 and 100%
        if self.fee_rate > Decimal::one() {
            return Err(StdError::generic_err("Fee rate can't be higher than 100%"));
        }
        // If fee rate is not zero, then there must be some fee recipients and their
        // weights must sum to 100%
        if !self.fee_rate.is_zero()
            && self.fee_recipients.iter().map(|(_, p)| p).sum::<Decimal>() != Decimal::one()
        {
            return Err(StdError::generic_err(
                "Sum of fee recipient percentages must be 100%",
            ));
        }
        // Fee recipients should not contain zero weights
        if self.fee_recipients.iter().any(|(_, p)| p.is_zero()) {
            return Err(StdError::generic_err(
                "Fee recipient percentages must be greater than zero",
            ));
        }
        Ok(FeeConfig {
            fee_rate: self.fee_rate,
            fee_recipients: self
                .fee_recipients
                .iter()
                .map(|(addr, percentage)| Ok((deps.api.addr_validate(addr)?, *percentage)))
                .collect::<StdResult<Vec<_>>>()?,
        })
    }
}

impl FeeConfig<Addr> {
    /// Creates messages to transfer an `AssetList` of assets to the fee
    /// recipients.
    pub fn transfer_assets_msgs(&self, assets: &AssetList, env: &Env) -> StdResult<Vec<CosmosMsg>> {
        if self.fee_rate.is_zero() {
            return Ok(vec![]);
        }
        Ok(self
            .fee_recipients
            .iter()
            // Filter out the contract address because it's unnecessary to send fees to ourselves
            .filter(|(addr, _)| addr != env.contract.address)
            .map(|(addr, percentage)| {
                let assets: AssetList = assets
                    .into_iter()
                    .map(|asset| Asset::new(asset.info.clone(), asset.amount * *percentage))
                    .filter(|asset| !asset.amount.is_zero())
                    .collect::<Vec<_>>()
                    .into();
                assets.transfer_msgs(addr).map_err(|e| {
                    StdError::generic_err(format!(
                        "Failed to create transfer messages for AssetList {}. Error: {}",
                        assets, e
                    ))
                })
            })
            .collect::<StdResult<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect())
    }

    /// Calculates the fee from the input assets and returns messages to send
    /// them to the fee recipients.
    ///
    /// # Arguments
    /// * `assets` - The assets to take the fee from.
    ///
    /// # Returns
    /// * `Vec<CosmosMsg>` - The messages to send the fees to the fee
    ///   recipients.
    /// * `AssetList` - The assets after the fee has been taken.
    pub fn fee_msgs_from_assets(
        &self,
        assets: &AssetList,
        env: &Env,
    ) -> StdResult<(Vec<CosmosMsg>, AssetList)> {
        // Take fee from input assets and filter out zero amounts
        let fees: AssetList = assets
            .into_iter()
            .map(|asset| Asset::new(asset.info.clone(), asset.amount * self.fee_rate))
            .filter(|asset| !asset.amount.is_zero())
            .collect::<Vec<_>>()
            .into();

        let mut assets_after_fees = assets.clone();
        assets_after_fees.deduct_many(&fees).map_err(|e| {
            StdError::generic_err(format!(
                "Failed to deduct fees from AssetList {}. Error: {}",
                assets, e
            ))
        })?;

        // Send fee to fee recipients
        Ok((self.transfer_assets_msgs(&fees, env)?, assets_after_fees))
    }

    /// Calculates the fee from the input asset and returns messages to send it
    /// to the fee recipients.
    ///
    /// # Arguments
    /// * `asset` - The asset to take the fee from.
    ///
    /// # Returns
    /// * `Vec<CosmosMsg>` - The messages to send the fees to the fee
    ///   recipients.
    /// * `Asset` - The asset after the fee has been taken.
    pub fn fee_msgs_from_asset(
        &self,
        asset: Asset,
        env: &Env,
    ) -> StdResult<(Vec<CosmosMsg>, Asset)> {
        let (msgs, assets_after_fee) =
            self.fee_msgs_from_assets(&AssetList::from(vec![asset]), env)?;
        Ok((msgs, assets_after_fee.to_vec()[0].clone()))
    }

    /// Calculates the fee from the input coins and returns a message to send
    /// them to the fee recipients.
    ///
    /// # Arguments
    /// * `coins` - The coins to take the fee from.
    ///
    /// # Returns
    /// * `Vec<CosmosMsg>` - The messages to send the fees to the fee recipients.
    /// * `Coins` - The coins after the fee has been taken.
    pub fn fee_msgs_from_coins(
        &self,
        coins: &Coins,
        env: &Env,
    ) -> StdResult<(Vec<CosmosMsg>, Coins)> {
        let assets: AssetList = coins
            .into_iter()
            .map(Into::into)
            .collect::<Vec<Asset>>()
            .into();

        let (fee_msgs, assets_after_fees) = self.fee_msgs_from_assets(&assets, env)?;

        let coins_after_fees: Coins = assets_after_fees
            .into_iter()
            .map(|a| {
                a.clone().try_into().map_err(|e| {
                    StdError::generic_err(format!(
                        "Failed to convert Asset {} to Coin. Error: {}",
                        a, e
                    ))
                })
            })
            .collect::<StdResult<Vec<Coin>>>()?
            .try_into()?;

        Ok((fee_msgs, coins_after_fees))
    }

    /// Calculates the fee from the input coin and returns messages to send it
    /// to the fee recipients.
    ///
    /// # Arguments
    /// * `coin` - The coin to take the fee from.
    ///
    /// # Returns
    /// * `Vec<CosmosMsg>` - The messages to send the fees to the fee
    ///   recipients.
    /// * `Coin` - The asset after the fee has been taken.
    pub fn fee_msgs_from_coin(&self, coin: Coin, env: &Env) -> StdResult<(Vec<CosmosMsg>, Coin)> {
        let (msgs, coins_after_fee) =
            self.fee_msgs_from_coins(&Coins::try_from(vec![coin.clone()])?, env)?;
        Ok((
            msgs,
            cosmwasm_std::coin(coins_after_fee.amount_of(&coin.denom).u128(), coin.denom),
        ))
    }
}

impl From<FeeConfig<Addr>> for FeeConfig<String> {
    fn from(value: FeeConfig<Addr>) -> Self {
        Self {
            fee_rate: value.fee_rate,
            fee_recipients: value
                .fee_recipients
                .into_iter()
                .map(|(addr, percentage)| (addr.to_string(), percentage))
                .collect(),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use cosmwasm_std::testing::{mock_dependencies, mock_env};
    use cosmwasm_std::{coin, Addr, BankMsg, Coins, CosmosMsg, Decimal, Uint128};
    use cw_asset::{Asset, AssetInfo, AssetList};

    #[test]
    fn fee_config_rate_cannot_be_larger_than_one() {
        let deps = mock_dependencies();

        let fee_config = super::FeeConfig {
            fee_rate: Decimal::one() + Decimal::percent(1),
            fee_recipients: vec![],
        };
        assert!(fee_config
            .check(&deps.as_ref())
            .unwrap_err()
            .to_string()
            .contains("Fee rate can't be higher than 100%"));
    }

    #[test]
    fn fee_config_recipients_must_sum_to_one() {
        let deps = mock_dependencies();

        let fee_config = super::FeeConfig {
            fee_rate: Decimal::percent(1),
            fee_recipients: vec![
                ("addr1".to_string(), Decimal::percent(20)),
                ("addr2".to_string(), Decimal::percent(50)),
            ],
        };
        assert!(fee_config
            .check(&deps.as_ref())
            .unwrap_err()
            .to_string()
            .contains("Sum of fee recipient percentages must be 100%"));
    }

    #[test]
    fn fee_config_recipient_weights_must_be_greater_than_zero() {
        let deps = mock_dependencies();

        let fee_config = super::FeeConfig {
            fee_rate: Decimal::percent(1),
            fee_recipients: vec![
                ("addr1".to_string(), Decimal::percent(100)),
                ("addr2".to_string(), Decimal::zero()),
            ],
        };
        assert!(fee_config
            .check(&deps.as_ref())
            .unwrap_err()
            .to_string()
            .contains("Fee recipient percentages must be greater than zero"));
    }

    #[test]
    fn fee_msgs_from_asset_works() {
        let env = mock_env();

        let fee_config = super::FeeConfig {
            fee_rate: Decimal::percent(1),
            fee_recipients: vec![(Addr::unchecked("addr1"), Decimal::percent(100))],
        };
        let asset = Asset::new(AssetInfo::native("uusdc"), 100u128);
        let (msgs, asset_after_fee) = fee_config.fee_msgs_from_asset(asset, &env).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            msgs[0],
            CosmosMsg::Bank(BankMsg::Send {
                to_address: "addr1".to_string(),
                amount: vec![coin(1u128, "uusdc".to_string())]
            })
        );
        assert_eq!(asset_after_fee.amount, Uint128::new(99));
    }

    #[test]
    fn fee_msgs_from_coin_works() {
        let env = mock_env();

        let fee_config = super::FeeConfig {
            fee_rate: Decimal::percent(1),
            fee_recipients: vec![(Addr::unchecked("addr1"), Decimal::percent(100))],
        };
        let coin = coin(100u128, "uusdc");
        let (msgs, coin_after_fee) = fee_config.fee_msgs_from_coin(coin.clone(), &env).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            msgs[0],
            CosmosMsg::Bank(BankMsg::Send {
                to_address: "addr1".to_string(),
                amount: vec![cosmwasm_std::coin(1u128, "uusdc".to_string())]
            })
        );
        assert_eq!(coin_after_fee, cosmwasm_std::coin(99u128, "uusdc"));
    }

    #[test]
    fn fee_msg_from_coins_works() {
        let env = mock_env();

        let fee_config = super::FeeConfig {
            fee_rate: Decimal::percent(1),
            fee_recipients: vec![(Addr::unchecked("addr1"), Decimal::percent(100))],
        };
        let coins = Coins::try_from(vec![coin(100u128, "uusdc")]).unwrap();
        let (msgs, coins_after_fee) = fee_config.fee_msgs_from_coins(&coins, &env).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            msgs[0],
            CosmosMsg::Bank(BankMsg::Send {
                to_address: "addr1".to_string(),
                amount: vec![coin(1u128, "uusdc".to_string())]
            })
        );
        assert_eq!(
            coins_after_fee,
            Coins::try_from(vec![coin(99u128, "uusdc")]).unwrap()
        );
    }

    #[test]
    fn fee_msgs_from_asset_works_with_zero_fee_rate() {
        let env = mock_env();

        let fee_config = super::FeeConfig {
            fee_rate: Decimal::zero(),
            fee_recipients: vec![],
        };
        let asset = Asset::new(AssetInfo::native("uusdc"), 100u128);
        let (msgs, asset_after_fee) = fee_config.fee_msgs_from_asset(asset, &env).unwrap();
        assert_eq!(msgs.len(), 0);
        assert_eq!(asset_after_fee.amount, Uint128::new(100));
    }

    #[test]
    fn fee_msg_from_coins_works_with_zero_fee_rate() {
        let env = mock_env();

        let fee_config = super::FeeConfig {
            fee_rate: Decimal::zero(),
            fee_recipients: vec![],
        };
        let coins = Coins::try_from(vec![coin(100u128, "uusdc")]).unwrap();
        let (msgs, coins_after_fee) = fee_config.fee_msgs_from_coins(&coins, &env).unwrap();
        assert_eq!(msgs.len(), 0);
        assert_eq!(
            coins_after_fee,
            Coins::try_from(vec![coin(100u128, "uusdc")]).unwrap()
        );
    }

    #[test]
    fn fee_msgs_from_assets_works() {
        let env = mock_env();

        let fee_config = super::FeeConfig {
            fee_rate: Decimal::percent(1),
            fee_recipients: vec![(Addr::unchecked("addr1"), Decimal::percent(100))],
        };
        let assets = vec![
            Asset::new(AssetInfo::native("uusdc"), 100u128),
            Asset::new(AssetInfo::native("uatom"), 200u128),
        ]
        .into();
        let (msgs, assets_after_fee) = fee_config.fee_msgs_from_assets(&assets, &env).unwrap();
        println!("{:?}", msgs);
        assert_eq!(msgs.len(), 2);
        assert_eq!(
            msgs[0],
            CosmosMsg::Bank(BankMsg::Send {
                to_address: "addr1".to_string(),
                amount: vec![coin(1u128, "uusdc".to_string())]
            })
        );
        assert_eq!(
            msgs[1],
            CosmosMsg::Bank(BankMsg::Send {
                to_address: "addr1".to_string(),
                amount: vec![coin(2u128, "uatom".to_string())]
            })
        );
        assert_eq!(assets_after_fee.to_vec()[0].amount, Uint128::new(99));
        assert_eq!(assets_after_fee.to_vec()[1].amount, Uint128::new(198));
    }

    #[test]
    fn fee_msgs_from_assets_works_with_multiple_recipients() {
        let env = mock_env();

        let fee_config = super::FeeConfig {
            fee_rate: Decimal::percent(1),
            fee_recipients: vec![
                (Addr::unchecked("addr1"), Decimal::percent(50)),
                (Addr::unchecked("addr2"), Decimal::percent(50)),
            ],
        };
        let assets = vec![
            Asset::new(AssetInfo::native("uusdc"), 1000u128),
            Asset::new(AssetInfo::native("uatom"), 2000u128),
        ]
        .into();
        let (msgs, assets_after_fee) = fee_config.fee_msgs_from_assets(&assets, &env).unwrap();
        assert_eq!(msgs.len(), 4);
        assert_eq!(
            msgs[0],
            CosmosMsg::Bank(BankMsg::Send {
                to_address: "addr1".to_string(),
                amount: vec![coin(5u128, "uusdc".to_string())]
            })
        );
        assert_eq!(
            msgs[1],
            CosmosMsg::Bank(BankMsg::Send {
                to_address: "addr1".to_string(),
                amount: vec![coin(10u128, "uatom".to_string())]
            })
        );
        assert_eq!(
            msgs[2],
            CosmosMsg::Bank(BankMsg::Send {
                to_address: "addr2".to_string(),
                amount: vec![coin(5u128, "uusdc".to_string())]
            })
        );
        assert_eq!(
            msgs[3],
            CosmosMsg::Bank(BankMsg::Send {
                to_address: "addr2".to_string(),
                amount: vec![coin(10u128, "uatom".to_string())]
            })
        );
        assert_eq!(assets_after_fee.to_vec()[0].amount, Uint128::new(990));
        assert_eq!(assets_after_fee.to_vec()[1].amount, Uint128::new(1980));
    }

    #[test]
    fn fee_msgs_from_coins_works_with_multiple_recipients() {
        let env = mock_env();

        let fee_config = super::FeeConfig {
            fee_rate: Decimal::percent(1),
            fee_recipients: vec![
                (Addr::unchecked("addr1"), Decimal::percent(50)),
                (Addr::unchecked("addr2"), Decimal::percent(50)),
            ],
        };
        let coins =
            Coins::try_from(vec![coin(1000u128, "uusdc"), coin(2000u128, "uatom")]).unwrap();
        let (msgs, coins_after_fee) = fee_config.fee_msgs_from_coins(&coins, &env).unwrap();
        assert_eq!(msgs.len(), 4);
        assert!(msgs.contains(&CosmosMsg::Bank(BankMsg::Send {
            to_address: "addr1".to_string(),
            amount: vec![coin(5u128, "uusdc".to_string())]
        })));
        assert!(msgs.contains(&CosmosMsg::Bank(BankMsg::Send {
            to_address: "addr1".to_string(),
            amount: vec![coin(10u128, "uatom".to_string())]
        })));
        assert!(msgs.contains(&CosmosMsg::Bank(BankMsg::Send {
            to_address: "addr2".to_string(),
            amount: vec![coin(5u128, "uusdc".to_string())]
        })));
        assert!(msgs.contains(&CosmosMsg::Bank(BankMsg::Send {
            to_address: "addr2".to_string(),
            amount: vec![coin(10u128, "uatom".to_string())]
        })));
        assert_eq!(
            coins_after_fee,
            Coins::try_from(vec![coin(990u128, "uusdc"), coin(1980u128, "uatom")]).unwrap()
        );
    }

    #[test]
    fn fee_msgs_from_assets_works_with_zero_fee_rate() {
        let env = mock_env();

        let fee_config = super::FeeConfig {
            fee_rate: Decimal::zero(),
            fee_recipients: vec![],
        };
        let assets = vec![
            Asset::new(AssetInfo::native("uusdc"), 100u128),
            Asset::new(AssetInfo::native("uatom"), 200u128),
        ]
        .into();
        let (msgs, assets_after_fee) = fee_config.fee_msgs_from_assets(&assets, &env).unwrap();
        assert_eq!(msgs.len(), 0);
        assert_eq!(assets_after_fee.to_vec()[0].amount, Uint128::new(100));
        assert_eq!(assets_after_fee.to_vec()[1].amount, Uint128::new(200));
    }

    #[test]
    fn fee_msgs_from_assets_works_when_asset_list_contains_zero_amounts() {
        let env = mock_env();

        let fee_config = super::FeeConfig {
            fee_rate: Decimal::percent(1),
            fee_recipients: vec![(Addr::unchecked("addr1"), Decimal::percent(100))],
        };
        let assets: AssetList = vec![
            Asset::native("uusdc", 100u128),
            Asset::native("uatom", 0u128),
        ]
        .into();
        let (msgs, coins_after_fee) = fee_config.fee_msgs_from_assets(&assets, &env).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            msgs[0],
            CosmosMsg::Bank(BankMsg::Send {
                to_address: "addr1".to_string(),
                amount: vec![coin(1u128, "uusdc".to_string())]
            })
        );
        assert_eq!(coins_after_fee, vec![Asset::native("uusdc", 99u128)].into());
    }
}
