use anyhow::{bail, Context, Result};
use std::{fs, path::PathBuf, sync::Arc};

pub trait NodeSecretStore: Send + Sync {
    fn encryption_root_key(&self) -> Result<Arc<[u8]>>;
}

#[derive(Clone, Default)]
pub struct EnvironmentSecretStore;

impl NodeSecretStore for EnvironmentSecretStore {
    fn encryption_root_key(&self) -> Result<Arc<[u8]>> {
        let value = if let Some(path) = std::env::var_os("UGOITE_NODE_SECRET_FILE") {
            fs::read(PathBuf::from(path)).context("read UGOITE_NODE_SECRET_FILE")?
        } else if let Some(value) = std::env::var_os("UGOITE_NODE_SECRET_KEY") {
            value.to_string_lossy().as_bytes().to_vec()
        } else {
            bail!(
                "UGOITE_NODE_SECRET_KEY or UGOITE_NODE_SECRET_FILE is required for node identity"
            );
        };
        let value = value
            .into_iter()
            .take_while(|byte| *byte != b'\n' && *byte != b'\r')
            .collect::<Vec<_>>();
        if value.len() < 32 {
            bail!("node secret must contain at least 32 bytes");
        }
        Ok(Arc::from(value))
    }
}

#[cfg(test)]
#[derive(Clone)]
pub struct TestSecretStore(pub Arc<[u8]>);

#[cfg(test)]
impl Default for TestSecretStore {
    fn default() -> Self {
        Self(Arc::from([0x5a; 32]))
    }
}

#[cfg(test)]
impl NodeSecretStore for TestSecretStore {
    fn encryption_root_key(&self) -> Result<Arc<[u8]>> {
        Ok(self.0.clone())
    }
}
