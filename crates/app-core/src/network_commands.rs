use crate::{AppState, State};
use core_model::{
    AppError,
    network_profiles::{NetworkProfile, validate_network_profile},
};

pub fn list_network_profiles(state: State<'_, AppState>) -> Result<Vec<NetworkProfile>, AppError> {
    state
        .database
        .list_network_profiles()
        .map_err(|error| AppError::storage(error.to_string()))
}

pub fn upsert_network_profile(
    profile: NetworkProfile,
    state: State<'_, AppState>,
) -> Result<NetworkProfile, AppError> {
    validate_network_profile(&profile)
        .map_err(|error| AppError::new("network_profile_invalid", error, true))?;
    state
        .database
        .upsert_network_profile(&profile)
        .map_err(|error| AppError::storage(error.to_string()))?;
    Ok(profile)
}

pub fn delete_network_profile(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    if id.trim().is_empty() || id.len() > 120 || id.chars().any(char::is_control) {
        return Err(AppError::new(
            "network_profile_invalid",
            "Network profile ID is invalid.",
            true,
        ));
    }
    state
        .database
        .delete_network_profile(&id)
        .map_err(|error| AppError::storage(error.to_string()))
}

pub fn disable_all_network_profiles(state: State<'_, AppState>) -> Result<usize, AppError> {
    state
        .database
        .disable_all_network_profiles()
        .map_err(|error| AppError::storage(error.to_string()))
}
