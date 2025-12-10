use miette::{Context, IntoDiagnostic, Result};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::PostgresVersion;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceState {
    pub container_id: String,

    pub postgres_version: PostgresVersion,

    pub port: u16,

    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct State {
    #[serde(default)]
    instances: HashMap<String, InstanceState>,
}
impl State {
    fn new() -> Result<Self> {
        let state_path = state_file_path()?;

        if !state_path.exists() {
            if let Some(parent) = state_path.parent() {
                std::fs::create_dir_all(parent)
                    .into_diagnostic()
                    .wrap_err("Failed to create .pgd directory")?;
            }

            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&state_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to read state file: {}", state_path.display()))?;

        let state: Self = serde_json::from_str(&content)
            .into_diagnostic()
            .wrap_err("Failed to parse state.json")?;

        Ok(state)
    }

    fn save(&self) -> Result<()> {
        let state_path = state_file_path()?;

        if let Some(parent) = state_path.parent() {
            std::fs::create_dir_all(parent)
                .into_diagnostic()
                .wrap_err("Failed to create .pgd directory")?;
        }

        let content = serde_json::to_string_pretty(self)
            .into_diagnostic()
            .wrap_err("Failed to serialize state")?;

        std::fs::write(&state_path, content)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to write state file: {}", state_path.display()))?;

        Ok(())
    }
}

pub struct StateManager(RefCell<State>);

impl StateManager {
    pub fn new() -> Result<Self> {
        Ok(Self(RefCell::new(State::new()?)))
    }

    pub fn save(&self) -> Result<()> {
        self.0.borrow().save()?;
        Ok(())
    }

    pub fn get(&self, project_name: &str) -> Option<InstanceState> {
        self.0.borrow().instances.get(project_name).cloned()
    }

    pub fn upsert(&self, project_name: String, state: InstanceState) {
        self.0.borrow_mut().instances.insert(project_name, state);
    }

    pub fn remove(&self, project_name: &str) -> Option<InstanceState> {
        self.0.borrow_mut().instances.remove(project_name)
    }

    pub fn get_highest_used_port(&self) -> Option<u16> {
        self.0.borrow().instances.values().map(|i| i.port).max()
    }
}

impl InstanceState {
    pub fn new(container_id: String, postgres_version: PostgresVersion, port: u16) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        InstanceState {
            container_id,
            postgres_version,
            port,
            created_at: now,
        }
    }
}

fn state_file_path() -> Result<PathBuf> {
    let home = std::env::home_dir().wrap_err("Failed to get HOME environment variable")?;

    Ok(home.join(".pgd").join("state.json"))
}
