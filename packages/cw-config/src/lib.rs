use cosmwasm_std::{Deps, DepsMut, Event, MessageInfo, Response, StdError, StdResult};
use cw_storage_plus::Item;
use optional_struct::Applyable;
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;
use thiserror::Error;

pub trait Validateable<T> {
    fn validate(&self, deps: &Deps) -> StdResult<T>;
}

/// Updates the config of the contract
pub fn update_config<T: Serialize + DeserializeOwned, U: From<T> + Validateable<T>>(
    deps: DepsMut,
    info: &MessageInfo,
    config_item: Item<T>,
    updates: impl Applyable<U> + Debug,
) -> Result<Response, ConfigError> {
    // Validate that the sender is the owner
    cw_ownable::assert_owner(deps.storage, &info.sender)?;

    let event = Event::new("sturdy/yield-split/update-config")
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

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("{0}")]
    StdError(#[from] StdError),

    #[error("{0}")]
    OwnershipError(#[from] cw_ownable::OwnershipError),

    #[error("Invalid config: {reason}")]
    InvalidConfig { reason: String },
}
