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

// === Rocksky credentials ===

const ROCKSKY_API_KEY: &str = "rocksky_api_key";
const ROCKSKY_SHARED_SECRET: &str = "rocksky_shared_secret";
const ROCKSKY_SESSION_KEY: &str = "rocksky_session_key";

/// Store Rocksky credentials in the system keyring
pub fn store_rocksky_credentials(
    api_key: &str,
    shared_secret: &str,
    session_key: &str,
) -> Result<(), keyring::Error> {
    Entry::new(SERVICE_NAME, ROCKSKY_API_KEY)?.set_password(api_key)?;
    Entry::new(SERVICE_NAME, ROCKSKY_SHARED_SECRET)?.set_password(shared_secret)?;
    Entry::new(SERVICE_NAME, ROCKSKY_SESSION_KEY)?.set_password(session_key)?;
    Ok(())
}

/// Retrieve Rocksky credentials from the system keyring.
/// Returns None if any credential is missing.
pub fn get_rocksky_credentials() -> Result<Option<(String, String, String)>, keyring::Error> {
    let api_key = match Entry::new(SERVICE_NAME, ROCKSKY_API_KEY)?.get_password() {
        Ok(v) => v,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(e) => return Err(e),
    };
    let shared_secret = match Entry::new(SERVICE_NAME, ROCKSKY_SHARED_SECRET)?.get_password() {
        Ok(v) => v,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(e) => return Err(e),
    };
    let session_key = match Entry::new(SERVICE_NAME, ROCKSKY_SESSION_KEY)?.get_password() {
        Ok(v) => v,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(e) => return Err(e),
    };
    Ok(Some((api_key, shared_secret, session_key)))
}

/// Delete Rocksky credentials from the system keyring
pub fn delete_rocksky_credentials() -> Result<(), keyring::Error> {
    for key in [ROCKSKY_API_KEY, ROCKSKY_SHARED_SECRET, ROCKSKY_SESSION_KEY] {
        let entry = Entry::new(SERVICE_NAME, key)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
