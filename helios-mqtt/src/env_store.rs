use std::collections::BTreeMap;

use helios_store::DocumentStore;
use serde::{Deserialize, Serialize};

const SHADOW_ENV_STATE_PATH: &str = "mqtt/shadow-env.json";

#[derive(Clone, Debug)]
pub struct ShadowEnvStore {
    store: DocumentStore,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedShadowEnv {
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub version: Option<i64>,
}

impl ShadowEnvStore {
    pub fn new(store: DocumentStore) -> Self {
        Self { store }
    }

    pub async fn load(&self) -> Result<PersistedShadowEnv, helios_store::Error> {
        Ok(self
            .store
            .get(SHADOW_ENV_STATE_PATH)
            .await?
            .unwrap_or_default())
    }

    pub async fn save(&self, state: &PersistedShadowEnv) -> Result<(), helios_store::Error> {
        self.store.put(SHADOW_ENV_STATE_PATH, state).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn it_round_trips_persisted_shadow_env() {
        let dir = tempdir().unwrap();
        let store = DocumentStore::with_root(dir.path()).await.unwrap();
        let env_store = ShadowEnvStore::new(store);

        let state = PersistedShadowEnv {
            env: [("FOO".to_string(), "bar".to_string())].into(),
            version: Some(7),
        };

        env_store.save(&state).await.unwrap();
        let restored = env_store.load().await.unwrap();

        assert_eq!(restored, state);
    }
}
