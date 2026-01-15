// SPDX-License-Identifier: MPL-2.0

//! Secure credential storage using the system keyring.
//!
//! This module stores sensitive credentials (like OAuth tokens) in the
//! system's secure credential store (GNOME Keyring, KDE Wallet, etc.)
//! instead of in plain config files. This ensures tokens survive config
//! version changes and are encrypted at rest.

use keyring::Entry;

const SERVICE_NAME: &str = "com.github.orta.cosmic-soundcloud";
const TOKEN_KEY: &str = "oauth_token";

/// Store the OAuth token in the system keyring
pub fn store_token(token: &str) -> Result<(), keyring::Error> {
    eprintln!("[keyring] store_token: creating entry for service={SERVICE_NAME}, key={TOKEN_KEY}");
    let entry = match Entry::new(SERVICE_NAME, TOKEN_KEY) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[keyring] store_token: Entry::new failed: {e}");
            return Err(e);
        }
    };
    eprintln!("[keyring] store_token: setting password (len={})", token.len());
    match entry.set_password(token) {
        Ok(()) => {
            eprintln!("[keyring] store_token: success");
            Ok(())
        }
        Err(e) => {
            eprintln!("[keyring] store_token: set_password failed: {e}");
            Err(e)
        }
    }
}

/// Retrieve the OAuth token from the system keyring
pub fn get_token() -> Result<Option<String>, keyring::Error> {
    eprintln!("[keyring] get_token: creating entry...");
    let entry = match Entry::new(SERVICE_NAME, TOKEN_KEY) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[keyring] get_token: Entry::new failed: {e}");
            return Err(e);
        }
    };
    eprintln!("[keyring] get_token: getting password...");
    match entry.get_password() {
        Ok(token) => {
            eprintln!("[keyring] get_token: got token (len={})", token.len());
            Ok(Some(token))
        }
        Err(keyring::Error::NoEntry) => {
            eprintln!("[keyring] get_token: NoEntry");
            Ok(None)
        }
        Err(e) => {
            eprintln!("[keyring] get_token: error: {e}");
            Err(e)
        }
    }
}

/// Delete the OAuth token from the system keyring
pub fn delete_token() -> Result<(), keyring::Error> {
    let entry = Entry::new(SERVICE_NAME, TOKEN_KEY)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()), // Already deleted
        Err(e) => Err(e),
    }
}

/// Check if a token exists in the keyring
pub fn has_token() -> bool {
    get_token().map(|t| t.is_some()).unwrap_or(false)
}
