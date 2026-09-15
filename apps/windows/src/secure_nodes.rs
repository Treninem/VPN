use amri_secrets::{SecretStore, SecretValue, WindowsDpapiSecretStore};
use amri_subscriptions::ImportedNode;
use std::path::PathBuf;
use zeroize::Zeroizing;

const IMPORTED_NODES_KEY: &str = "subscriptions:imported-nodes:v1";

fn store() -> Result<WindowsDpapiSecretStore, String> {
    let root = std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "Windows local application data directory is unavailable".to_string())?
        .join("AMRI VPN")
        .join("secrets");
    Ok(WindowsDpapiSecretStore::new(root))
}

pub fn load() -> Result<Vec<ImportedNode>, String> {
    let Some(secret) = store()?
        .get(IMPORTED_NODES_KEY)
        .map_err(|_| "secure node storage could not be opened".to_string())?
    else {
        return Ok(Vec::new());
    };
    serde_json::from_slice(secret.expose_secret())
        .map_err(|_| "secure node storage contains invalid data".to_string())
}

pub fn save(nodes: &[ImportedNode]) -> Result<(), String> {
    let store = store()?;
    if nodes.is_empty() {
        store
            .delete(IMPORTED_NODES_KEY)
            .map(|_| ())
            .map_err(|_| "secure node storage could not be cleared".to_string())
    } else {
        let serialized = Zeroizing::new(
            serde_json::to_string(nodes)
                .map_err(|_| "imported nodes could not be encoded".to_string())?,
        );
        let secret = SecretValue::from_text(serialized.as_str())
            .map_err(|_| "imported nodes could not be protected".to_string())?;
        store
            .put(IMPORTED_NODES_KEY, &secret)
            .map_err(|_| "secure node storage could not be updated".to_string())
    }
}
