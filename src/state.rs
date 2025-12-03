use miette::{Context, IntoDiagnostic, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// State information for a single PostgreSQL instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceState {
    /// Docker container ID
    pub container_id: String,

    /// PostgreSQL version running in the container
    pub postgres_version: String,

    /// Database name
    pub database_name: String,

    /// User name
    pub user_name: String,

    /// Port the container is bound to
    pub port: u16,

    /// Timestamp when the instance was created (Unix timestamp)
    pub created_at: u64,

    /// Timestamp when the instance was last started (Unix timestamp)
    pub last_started_at: Option<u64>,
}

/// Manages the global state file at ~/.pgx/state.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateManager {
    /// Map of project name to instance state
    #[serde(default)]
    instances: HashMap<String, InstanceState>,
}

/// Get the path to the state file (~/.pgx/state.json)

fn state_file_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .into_diagnostic()
        .wrap_err("Failed to get HOME environment variable")?;

    Ok(PathBuf::from(home).join(".pgx").join("state.json"))
}

/// Get the path to the .pgx directory
pub fn pgx_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .into_diagnostic()
        .wrap_err("Failed to get HOME environment variable")?;

    Ok(PathBuf::from(home).join(".pgx"))
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
                    .wrap_err("Failed to create .pgx directory")?;
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
                .wrap_err("Failed to create .pgx directory")?;
        }

        let content = serde_json::to_string_pretty(self)
            .into_diagnostic()
            .wrap_err("Failed to serialize state")?;

        std::fs::write(&state_path, content)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to write state file: {}", state_path.display()))?;

        Ok(())
    }

    /// Get the state for a specific project
    pub fn get(&self, project_name: &str) -> Option<&InstanceState> {
        self.instances.get(project_name)
    }

    /// Get mutable state for a specific project
    pub fn get_mut(&mut self, project_name: &str) -> Option<&mut InstanceState> {
        self.instances.get_mut(project_name)
    }

    /// Set the state for a specific project
    pub fn set(&mut self, project_name: String, state: InstanceState) {
        self.instances.insert(project_name, state);
    }

    /// Update the state for a specific project, creating it if it doesn't exist
    pub fn update<F>(&mut self, project_name: &str, updater: F) -> Result<()>
    where
        F: FnOnce(&mut InstanceState),
    {
        if let Some(state) = self.instances.get_mut(project_name) {
            updater(state);
            Ok(())
        } else {
            miette::bail!("No state found for project: {}", project_name)
        }
    }

    /// Remove the state for a specific project
    pub fn remove(&mut self, project_name: &str) -> Option<InstanceState> {
        self.instances.remove(project_name)
    }

    /// Get all instances
    pub fn all_instances(&self) -> &HashMap<String, InstanceState> {
        &self.instances
    }
}

impl InstanceState {
    pub fn new(
        container_id: String,
        postgres_version: String,
        database_name: String,
        user_name: String,
        port: u16,
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        InstanceState {
            container_id,
            postgres_version,
            database_name,
            user_name,
            port,
            created_at: now,
            last_started_at: Some(now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_manager_operations() {
        let mut manager = StateManager {
            instances: HashMap::new(),
        };

        let state = InstanceState::new(
            "container123".to_string(),
            "16".to_string(),
            "mydb".to_string(),
            "postgres".to_string(),
            5432,
        );

        manager.set("my-project".to_string(), state);

        assert!(manager.get("my-project").is_some());
        assert_eq!(
            manager.get("my-project").unwrap().container_id,
            "container123"
        );

        manager.remove("my-project");
        assert!(manager.get("my-project").is_none());
    }
}
