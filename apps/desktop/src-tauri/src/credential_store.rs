use keyring::{Entry, Error};
use zeroize::Zeroizing;

use crate::settings::RuntimeSecretKind;

const SSH_PASSWORD_SERVICE: &str = "dev.lumensource.desktop.ssh";
const RUNTIME_SECRET_ACCOUNT: &str = "default";

fn entry(target_id: &str) -> Result<Entry, Error> {
    Entry::new(SSH_PASSWORD_SERVICE, target_id)
}

fn storage_error(action: &str, error: impl std::fmt::Display) -> String {
    format!("Could not {action} the SSH password in the operating system credential store: {error}")
}

pub async fn save_password(target_id: String, password: Zeroizing<String>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let entry = entry(&target_id).map_err(|error| storage_error("open", error))?;
        entry
            .set_password(password.as_str())
            .map_err(|error| storage_error("save", error))
    })
    .await
    .map_err(|error| format!("The SSH credential-store task failed: {error}"))?
}

pub async fn load_password(target_id: String) -> Result<Option<Zeroizing<String>>, String> {
    tokio::task::spawn_blocking(move || {
        let entry = entry(&target_id).map_err(|error| storage_error("open", error))?;
        match entry.get_password() {
            Ok(password) => Ok(Some(Zeroizing::new(password))),
            Err(Error::NoEntry) => Ok(None),
            Err(error) => Err(storage_error("read", error)),
        }
    })
    .await
    .map_err(|error| format!("The SSH credential-store task failed: {error}"))?
}

pub async fn password_is_saved(target_id: String) -> Result<bool, String> {
    Ok(load_password(target_id).await?.is_some())
}

pub async fn delete_password(target_id: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let entry = entry(&target_id).map_err(|error| storage_error("open", error))?;
        match entry.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(storage_error("delete", error)),
        }
    })
    .await
    .map_err(|error| format!("The SSH credential-store task failed: {error}"))?
}

pub async fn save_runtime_secret(
    kind: RuntimeSecretKind,
    secret: Zeroizing<String>,
) -> Result<(), String> {
    save_runtime_secret_for_account(kind, RUNTIME_SECRET_ACCOUNT.to_owned(), secret).await
}

pub async fn save_runtime_secret_for_account(
    kind: RuntimeSecretKind,
    account: String,
    secret: Zeroizing<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let entry = Entry::new(kind.service_name(), &account)
            .map_err(|error| secret_storage_error(kind, "open", error))?;
        entry
            .set_password(secret.as_str())
            .map_err(|error| secret_storage_error(kind, "save", error))
    })
    .await
    .map_err(|error| format!("The runtime credential-store task failed: {error}"))?
}

pub async fn runtime_secret_is_saved(kind: RuntimeSecretKind) -> Result<bool, String> {
    runtime_secret_is_saved_for_account(kind, RUNTIME_SECRET_ACCOUNT.to_owned()).await
}

pub async fn runtime_secret_is_saved_for_account(
    kind: RuntimeSecretKind,
    account: String,
) -> Result<bool, String> {
    Ok(load_runtime_secret_for_account(kind, account)
        .await?
        .is_some())
}

pub async fn load_runtime_secret_for_account(
    kind: RuntimeSecretKind,
    account: String,
) -> Result<Option<Zeroizing<String>>, String> {
    tokio::task::spawn_blocking(move || {
        let entry = Entry::new(kind.service_name(), &account)
            .map_err(|error| secret_storage_error(kind, "open", error))?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(Error::NoEntry) => Ok(None),
            Err(error) => Err(secret_storage_error(kind, "read", error)),
        }
    })
    .await
    .map_err(|error| format!("The runtime credential-store task failed: {error}"))?
}

pub async fn delete_runtime_secret(kind: RuntimeSecretKind) -> Result<(), String> {
    delete_runtime_secret_for_account(kind, RUNTIME_SECRET_ACCOUNT.to_owned()).await
}

pub async fn delete_runtime_secret_for_account(
    kind: RuntimeSecretKind,
    account: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let entry = Entry::new(kind.service_name(), &account)
            .map_err(|error| secret_storage_error(kind, "open", error))?;
        match entry.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(secret_storage_error(kind, "delete", error)),
        }
    })
    .await
    .map_err(|error| format!("The runtime credential-store task failed: {error}"))?
}

fn secret_storage_error(
    kind: RuntimeSecretKind,
    action: &str,
    error: impl std::fmt::Display,
) -> String {
    let credential = match kind {
        RuntimeSecretKind::VllmApiKey => "vLLM API key",
        RuntimeSecretKind::HuggingFaceToken => "Hugging Face token",
    };
    format!("Could not {action} the {credential} in the operating system credential store: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_name_is_application_scoped() {
        assert_eq!(SSH_PASSWORD_SERVICE, "dev.lumensource.desktop.ssh");
        assert_eq!(
            RuntimeSecretKind::VllmApiKey.service_name(),
            "dev.lumensource.desktop.vllm"
        );
    }
}
