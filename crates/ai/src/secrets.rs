//! API keys in the operating system's credential store (Windows Credential Manager, macOS
//! Keychain, the Secret Service on Linux), one per provider. Keys never go into settings files,
//! logs or the control channel.

use crate::provider::Provider;

/// The service name the keys are filed under.
const SERVICE: &str = "Docufy AI";

fn entry(provider: Provider) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, provider.id()).map_err(|e| format!("the credential store is not available: {e}"))
}

/// Stores `key` for `provider`, replacing any earlier one. A blank key deletes it.
pub fn store(provider: Provider, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return delete(provider);
    }
    entry(provider)?.set_password(key).map_err(|e| format!("the key could not be stored: {e}"))
}

/// The key stored for `provider`, if any.
pub fn load(provider: Provider) -> Result<Option<String>, String> {
    match entry(provider)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("the key could not be read: {e}")),
    }
}

/// Deletes the key stored for `provider`; nothing stored is not an error.
pub fn delete(provider: Provider) -> Result<(), String> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("the key could not be deleted: {e}")),
    }
}

/// A key shown in settings: the first and last four characters, so the user can tell keys apart
/// without the key being readable over their shoulder.
pub fn masked(key: &str) -> String {
    let chars: Vec<char> = key.trim().chars().collect();
    if chars.len() <= 12 {
        return "•".repeat(chars.len().max(8));
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}••••••••{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_masked() {
        assert_eq!(masked("sk-ant-api03-abcdefghijklmnopWXYZ"), "sk-a••••••••WXYZ");
        assert_eq!(masked("short"), "••••••••");
        assert!(!masked("sk-ant-api03-secretsecretsecret").contains("secret"));
    }
}
