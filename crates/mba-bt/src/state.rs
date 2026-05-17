use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use mba_protocol::BtClientRecord;
use serde::{Deserialize, Serialize};
use tracing::info;

const BT_STATE_SCHEMA_VERSION: u32 = 1;
const STATE_FILE_NAME: &str = "state.json";
const CLIENTS_DIR_NAME: &str = "clients";

#[derive(Debug, Clone)]
pub struct BtStateStore {
    root: PathBuf,
    trusted_client_count: usize,
}

impl BtStateStore {
    pub fn open(root: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let root = root.into();
        prepare_dir(&root, 0o700)?;

        let clients_dir = root.join(CLIENTS_DIR_NAME);
        prepare_dir(&clients_dir, 0o700)?;

        let state_path = root.join(STATE_FILE_NAME);
        if !state_path.exists() {
            let state = BtStateFile {
                schema_version: BT_STATE_SCHEMA_VERSION,
                created_unix_seconds: current_unix_seconds(),
            };
            let bytes =
                serde_json::to_vec_pretty(&state).context("failed to encode bt state metadata")?;
            std::fs::write(&state_path, bytes).with_context(|| {
                format!("failed to write bt state metadata {}", state_path.display())
            })?;
            std::fs::set_permissions(&state_path, std::fs::Permissions::from_mode(0o600))
                .with_context(|| {
                    format!("failed to chmod bt state metadata {}", state_path.display())
                })?;
        }

        let trusted_client_count = count_trusted_clients(&clients_dir)?;
        info!(
            state_dir = %root.display(),
            trusted_client_count,
            "opened bluetooth state store"
        );

        Ok(Self {
            root,
            trusted_client_count,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn trusted_client_count(&self) -> usize {
        self.trusted_client_count
    }

    pub fn list_clients(&self) -> anyhow::Result<Vec<BtClientRecord>> {
        let mut clients = read_client_records(&self.clients_dir())?;
        clients.sort_by(|left, right| left.client_id.cmp(&right.client_id));
        Ok(clients)
    }

    pub fn upsert_client(&mut self, client: &BtClientRecord) -> anyhow::Result<()> {
        let path = self.client_path(&client.client_id)?;
        let bytes =
            serde_json::to_vec_pretty(client).context("failed to encode bt client record")?;
        std::fs::write(&path, bytes)
            .with_context(|| format!("failed to write bt client record {}", path.display()))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to chmod bt client record {}", path.display()))?;
        self.refresh_trusted_client_count()?;
        Ok(())
    }

    pub fn forget_client(&mut self, client_id: &str) -> anyhow::Result<bool> {
        let path = self.client_path(client_id)?;
        match std::fs::remove_file(&path) {
            Ok(()) => {
                self.refresh_trusted_client_count()?;
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error)
                .with_context(|| format!("failed to remove bt client record {}", path.display())),
        }
    }

    fn client_path(&self, client_id: &str) -> anyhow::Result<PathBuf> {
        validate_client_id(client_id)?;
        Ok(self.clients_dir().join(format!("{client_id}.json")))
    }

    fn clients_dir(&self) -> PathBuf {
        self.root.join(CLIENTS_DIR_NAME)
    }

    fn refresh_trusted_client_count(&mut self) -> anyhow::Result<()> {
        self.trusted_client_count = count_trusted_clients(&self.clients_dir())?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BtStateFile {
    schema_version: u32,
    created_unix_seconds: u64,
}

fn prepare_dir(path: &Path, mode: u32) -> anyhow::Result<()> {
    std::fs::create_dir_all(path)
        .with_context(|| format!("failed to create bt state directory {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("failed to chmod bt state directory {}", path.display()))
}

fn count_trusted_clients(clients_dir: &Path) -> anyhow::Result<usize> {
    let mut count = 0;
    for client in read_client_records(clients_dir)? {
        if client.trusted {
            count += 1;
        }
    }
    Ok(count)
}

fn read_client_records(clients_dir: &Path) -> anyhow::Result<Vec<BtClientRecord>> {
    let mut clients = Vec::new();
    for entry in std::fs::read_dir(clients_dir).with_context(|| {
        format!(
            "failed to read bt clients directory {}",
            clients_dir.display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!(
                "failed to read bt clients directory entry {}",
                clients_dir.display()
            )
        })?;
        let path = entry.path();
        if !path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            continue;
        }

        let bytes = std::fs::read(&path)
            .with_context(|| format!("failed to read bt client record {}", path.display()))?;
        let client = serde_json::from_slice::<BtClientRecord>(&bytes)
            .with_context(|| format!("failed to decode bt client record {}", path.display()))?;
        clients.push(client);
    }
    Ok(clients)
}

fn validate_client_id(client_id: &str) -> anyhow::Result<()> {
    if client_id.is_empty() || client_id.len() > 96 {
        anyhow::bail!("client_id must be 1..=96 characters");
    }
    if !client_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        anyhow::bail!("client_id may contain only ASCII letters, digits, '.', '_', and '-'");
    }
    Ok(())
}

fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_store_initializes_metadata_and_clients_dir() {
        let root = unique_test_dir();
        let store = BtStateStore::open(&root).expect("state opens");

        assert_eq!(store.root(), root.as_path());
        assert_eq!(store.trusted_client_count(), 0);
        assert!(root.join(STATE_FILE_NAME).is_file());
        assert!(root.join(CLIENTS_DIR_NAME).is_dir());

        std::fs::remove_dir_all(root).expect("test dir cleanup");
    }

    #[test]
    fn state_store_lists_and_forgets_client_records() {
        let root = unique_test_dir();
        let mut store = BtStateStore::open(&root).expect("state opens");
        let client = BtClientRecord {
            schema_version: 1,
            client_id: "phone-1".to_string(),
            display_name: Some("Pixel 7 Pro".to_string()),
            trusted: true,
            created_unix_seconds: 1_765_000_000,
            last_seen_unix_seconds: Some(1_765_000_120),
            last_ble_address: Some("57:29:36:B6:FD:53".to_string()),
            protocol_version: Some(1),
        };

        store.upsert_client(&client).expect("client writes");

        assert_eq!(store.trusted_client_count(), 1);
        assert_eq!(store.list_clients().expect("clients list"), vec![client]);
        assert!(store.forget_client("phone-1").expect("client forgotten"));
        assert_eq!(store.trusted_client_count(), 0);
        assert!(store.list_clients().expect("clients list").is_empty());
        assert!(!store
            .forget_client("phone-1")
            .expect("missing client is not forgotten"));

        std::fs::remove_dir_all(root).expect("test dir cleanup");
    }

    #[test]
    fn client_id_rejects_path_traversal() {
        let root = unique_test_dir();
        let mut store = BtStateStore::open(&root).expect("state opens");

        assert!(store.forget_client("../nope").is_err());
        assert!(store.forget_client("nested/nope").is_err());

        std::fs::remove_dir_all(root).expect("test dir cleanup");
    }

    fn unique_test_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("mba-bt-state-test-{}-{nanos}", std::process::id()))
    }
}
