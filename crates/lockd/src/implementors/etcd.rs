use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use etcd_client::Client;
use futures::executor::block_on;
use runtime::resource::BasicState;

use crate::providers::etcd;

/// This is the underlying struct behind the `Etcd` variant of the `EtcdImplementor` enum.
///
/// It provides a properties that pertains solely to the etcd implementation
/// of this capability:
///     - `client`, and
///
/// As per its' usage in `EtcdImplementor`, it must `derive` `Debug`, and `Clone`.
#[derive(Clone)]
pub struct EtcdImplementor {
    client: Option<Arc<Mutex<Client>>>,
}

impl std::fmt::Debug for EtcdImplementor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EtcdImplementor")
    }
}

impl EtcdImplementor {
    pub fn new(slight_state: &BasicState) -> Self {
        let endpoint = String::from_utf8(
            runtime_configs::get(
                &slight_state.secret_store,
                "ETCD_ENDPOINT",
                &slight_state.config_toml_file_path,
            )
            .with_context(|| {
                format!(
                    "failed to get 'ETCD_ENDPOINT' secret using secret store type: {}",
                    slight_state.secret_store
                )
            })
            .unwrap(),
        )
        .unwrap();

        let client = block_on(Client::connect([endpoint], None))
            .with_context(|| "failed to connect to etcd server")
            .unwrap();
        // ^^^ from my tests with localhost client, this never fails
        Self {
            client: Some(Arc::new(Mutex::new(client))),
        }
    }

    pub fn lock(&self, lock_name: &[u8]) -> Result<Vec<u8>> {
        let inner = self.client.as_ref().unwrap();
        let pr = block_on(etcd::lock(&mut inner.lock().unwrap(), lock_name))
            .with_context(|| "failed to acquire lock")?;
        Ok(pr)
    }

    pub fn lock_with_time_to_live(
        &self,
        lock_name: &[u8],
        time_to_live_in_secs: i64,
    ) -> Result<Vec<u8>> {
        let inner = self.client.as_ref().unwrap();
        let pr = block_on(etcd::lock_with_lease(
            &mut inner.lock().unwrap(),
            lock_name,
            time_to_live_in_secs,
        ))
        .with_context(|| "failed to acquire lock with time to live")?;
        Ok(pr)
    }

    pub fn unlock(&self, lock_key: &[u8]) -> Result<()> {
        let inner = self.client.as_ref().unwrap();
        block_on(etcd::unlock(&mut inner.lock().unwrap(), lock_key))
            .with_context(|| "failed to unlock")?;
        Ok(())
    }
}
