use miette::{Context, IntoDiagnostic, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::PostgresVersion;

/// State information for a single PostgreSQL instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceState {
    /// Docker container ID
    pub container_id: String,

    /// PostgreSQL version running in the container
    pub postgres_version: PostgresVersion,

    /// Port the container is bound to
    pub port: u16,

    /// Timestamp when the instance was created (Unix timestamp)
    pub created_at: u64,
}

/// Manages the global state file at ~/.pgd/state.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateManager {
    /// Map of project name to instance state
    #[serde(default)]
    instances: HashMap<String, InstanceState>,
}

/// Get the path to the state file (~/.pgd/state.json)

fn state_file_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .into_diagnostic()
        .wrap_err("Failed to get HOME environment variable")?;

    Ok(PathBuf::from(home).join(".pgd").join("state.json"))
}


impl StateManager {
    /// Load the state manager from disk, or create a new one if it doesn't exist
    pub fn load() -> Result<Self> {
        let state_path = state_file_path()?;

        if !state_path.exists() {
            // Create the directory if it doesn't exist
            if let Some(parent) = state_path.parent() {
                std::fs::create_dir_all(parent)
                    .into_diagnostic()
                    .wrap_err("Failed to create .pgd directory")?;
            }

            // Return empty state
            return Ok(StateManager {
                instances: HashMap::new(),
            });
        }

        let content = std::fs::read_to_string(&state_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to read state file: {}", state_path.display()))?;

        let state: StateManager = serde_json::from_str(&content)
            .into_diagnostic()
            .wrap_err("Failed to parse state.json")?;

        Ok(state)
    }

    /// Save the state manager to disk
    pub fn save(&self) -> Result<()> {
        let state_path = state_file_path()?;

        // Ensure directory exists
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

    /// Get mutable state for a specific project
    pub fn get_mut(&mut self, project_name: &str) -> Option<&mut InstanceState> {
        self.instances.get_mut(project_name)
    }

    /// Set the state for a specific project
    pub fn set(&mut self, project_name: String, state: InstanceState) {
        self.instances.insert(project_name, state);
    }

    /// Remove the state for a specific project
    pub fn remove(&mut self, project_name: &str) -> Option<InstanceState> {
        self.instances.remove(project_name)
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