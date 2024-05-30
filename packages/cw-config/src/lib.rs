use cosmwasm_std::{
    Addr, Deps, DepsMut, Event, MessageInfo, Response, StdError, StdResult, Storage,
};
use cw_storage_plus::Item;
pub use optional_struct::Applyable;
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;
use thiserror::Error;

// Re-exports for convenience
pub use optional_struct;

pub trait Validateable<T> {
    fn validate(&self, deps: &Deps) -> StdResult<T>;
}

/// Updates the a config item with new values.
///
/// # Generics
///
/// * `T` - The type of the validated config.
/// * `U` - The type of the unvalidated config.
/// * `E` - The type of the error returned by the access check.
///
/// Requires that T implements `Serialize + DeserializeOwned`.
/// Requires that U implements `From<T> + Validateable<T>`. I.e. that the unvalidated config can be
/// validated into a validated config and that the unvalidated config can be created from a validated config.
///
/// # Arguments
///
/// * `deps` - The dependencies for querying the chain.
/// * `info` - The message info of the transaction.
/// * `config_item` - The item to load and save the config.
/// * `updates` - The updates to apply to the config.
/// * `access_allowed` - A function that checks if the sender is allowed to update the config.
///                If `None`, the sender is always allowed to update the config.
///                The function takes the storage and the sender address and returns an error if the sender is not allowed.
pub fn update_config<T: Serialize + DeserializeOwned, U: From<T> + Validateable<T>, E>(
    deps: DepsMut,
    info: &MessageInfo,
    config_item: Item<T>,
    updates: impl Applyable<U> + Debug,
    access_allowed: Option<impl FnOnce(&dyn Storage, &Addr) -> Result<(), E>>,
) -> Result<Response, ConfigError> {
    // Validate that the sender is the owner
    access_allowed
        .map(|check| check(deps.storage, &info.sender))
        .transpose()
        .map_err(|_| ConfigError::Unauthorized {})?;

    let event = Event::new("apollodao/cw-config/update-config")
        .add_attribute("updates", format!("{:?}", updates));

    // Load the old config, turn it into the unchecked version, apply the updates,
    // validate the new config and save it back to the item
    let config = config_item.load(deps.storage)?;
    let mut config_unchecked: U = config.into();
    updates.apply_to(&mut config_unchecked);
    let config = config_unchecked.validate(&deps.as_ref())?;
    config_item.save(deps.storage, &config)?;

    Ok(Response::new().add_event(event))
}

#[derive(Error, Debug, PartialEq)]
pub enum ConfigError {
    #[error("{0}")]
    StdError(#[from] StdError),

    #[error("{0}")]
    OwnershipError(#[from] cw_ownable::OwnershipError),

    #[error("Invalid config: {reason}")]
    InvalidConfig { reason: String },

    #[error("Unauthorized")]
    Unauthorized {},
}

#[cfg(test)]
mod tests {
    use std::borrow::BorrowMut;

    use crate::{update_config, ConfigError, Validateable};
    use cosmwasm_schema::schemars::JsonSchema;
    use cosmwasm_schema::serde::{Deserialize, Serialize};
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_info},
        Addr, StdError,
    };
    use cw_address_like::AddressLike;
    use cw_storage_plus::Item;
    use optional_struct::{optional_struct, Applyable};

    #[optional_struct(ConfigUpdates)]
    #[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
    pub struct ConfigBase<T: AddressLike> {
        pub example_addr: T,
    }

    pub type Config = ConfigBase<Addr>;
    pub type ConfigUnchecked = ConfigBase<String>;

    impl From<Config> for ConfigUnchecked {
        fn from(config: Config) -> Self {
            ConfigUnchecked {
                example_addr: config.example_addr.to_string(),
            }
        }
    }

    impl Validateable<Config> for ConfigUnchecked {
        fn validate(&self, deps: &cosmwasm_std::Deps) -> Result<Config, StdError> {
            Ok(Config {
                example_addr: deps.api.addr_validate(&self.example_addr)?,
            })
        }
    }

    const CONFIG: Item<Config> = Item::new("config");

    #[test]
    fn test_access_control() {
        let mut deps = mock_dependencies();

        // Instantiate owner
        let owner = Addr::unchecked("owner");
        cw_ownable::initialize_owner(deps.storage.borrow_mut(), &deps.api, Some(owner.as_str()))
            .unwrap();

        let config = Config {
            example_addr: Addr::unchecked("example"),
        };
        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        let updates = ConfigUpdates {
            example_addr: Some("example2".to_string()),
        };

        // Call from other sender, should fail
        let info = mock_info("sender", &[]);
        let err = update_config::<Config, ConfigUnchecked, _>(
            deps.as_mut(),
            &info,
            CONFIG,
            updates.clone(),
            Some(cw_ownable::assert_owner),
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::Unauthorized {}));

        // Call form owner, should succeed
        let info = mock_info(owner.as_str(), &[]);
        update_config::<Config, ConfigUnchecked, _>(
            deps.as_mut(),
            &info,
            CONFIG,
            updates,
            Some(cw_ownable::assert_owner),
        )
        .unwrap();
    }
}
