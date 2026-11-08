// SPDX-License-Identifier: AGPL-3.0-or-later

//! Where tokens come from: the environment, GitHub CLI, then the system keyring. They are never
//! written to the configuration or the cache.

use crate::source::github::GithubClient;

const SERVICE: &str = "pigeonnier";
pub const GITHUB: &str = "github";
pub const TODOIST: &str = "todoist";

#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("the system keyring refused: {0}")]
    Keyring(String),
    #[error("no system keyring on this platform")]
    Unsupported,
}

/// The store runs its own async runtime, which must not start inside the caller's: a plain thread
/// keeps these functions callable from anywhere.
#[cfg(target_os = "linux")]
fn with_entry<T: Send + 'static>(
    account: &str,
    action: impl FnOnce(&keyring_core::Entry) -> keyring_core::Result<T> + Send + 'static,
) -> Result<T, CredentialError> {
    static STORE: std::sync::Once = std::sync::Once::new();
    let account = account.to_owned();
    std::thread::spawn(move || {
        let failure = |error: keyring_core::Error| CredentialError::Keyring(error.to_string());
        let mut setup = Ok(());
        STORE.call_once(|| match zbus_secret_service_keyring_store::Store::new() {
            Ok(store) => keyring_core::set_default_store(store),
            Err(error) => setup = Err(failure(error)),
        });
        setup?;
        let entry = keyring_core::Entry::new(SERVICE, &account).map_err(failure)?;
        action(&entry).map_err(failure)
    })
    .join()
    .unwrap_or_else(|_| {
        Err(CredentialError::Keyring(
            "the keyring thread failed".to_owned(),
        ))
    })
}

#[cfg(not(target_os = "linux"))]
fn with_entry<T>(
    _account: &str,
    _action: impl FnOnce(&keyring_core::Entry) -> keyring_core::Result<T>,
) -> Result<T, CredentialError> {
    Err(CredentialError::Unsupported)
}

/// # Errors
/// The keyring is missing or refuses.
pub fn store(account: &str, token: &str) -> Result<(), CredentialError> {
    let token = token.to_owned();
    with_entry(account, move |entry| entry.set_password(&token))
}

/// Removes a token; one that was not there is not a failure.
///
/// # Errors
/// The keyring is missing or refuses.
pub fn forget(account: &str) -> Result<(), CredentialError> {
    with_entry(account, |entry| match entry.delete_credential() {
        Err(keyring_core::Error::NoEntry) => Ok(()),
        other => other,
    })
}

fn from_keyring(account: &str) -> Option<String> {
    with_entry(account, keyring_core::Entry::get_password).ok()
}

fn from_environment(variable: &str) -> Option<String> {
    std::env::var(variable)
        .ok()
        .filter(|value| !value.is_empty())
}

#[must_use]
pub fn github_token() -> Option<String> {
    GithubClient::token_from_gh_cli()
        .ok()
        .or_else(|| from_keyring(GITHUB))
}

#[must_use]
pub fn todoist_token() -> Option<String> {
    from_environment("TODOIST_TOKEN").or_else(|| from_keyring(TODOIST))
}
