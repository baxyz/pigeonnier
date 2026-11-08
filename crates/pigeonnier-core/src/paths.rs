// SPDX-License-Identifier: AGPL-3.0-or-later

//! XDG locations of the configuration and the cache.

use std::path::PathBuf;

fn xdg(variable: &str, fallback: &str) -> PathBuf {
    let base = std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map_or_else(
            || {
                let home = std::env::var_os("HOME").unwrap_or_default();
                PathBuf::from(home).join(fallback)
            },
            PathBuf::from,
        );
    base.join("pigeonnier")
}

#[must_use]
pub fn config_file() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("projects.toml")
}

#[must_use]
pub fn views_file() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("views.json")
}

#[must_use]
pub fn cache_file() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache").join("cache.sqlite")
}
