use keyring::{Entry, Error};
use zeroize::Zeroizing;

const SSH_PASSWORD_SERVICE: &str = "dev.lumensource.desktop.ssh";

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_name_is_application_scoped() {
        assert_eq!(SSH_PASSWORD_SERVICE, "dev.lumensource.desktop.ssh");
    }
}
