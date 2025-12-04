use miette::miette;
use miette::{Context, IntoDiagnostic, Result};
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};
use std::fmt::Display;
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PostgresVersion {
    pub major: u32,
    pub minor: u32,
}

impl FromStr for PostgresVersion {
    type Err = miette::Report;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let Some((major_str, minor_str)) = s.split_once(".") else {
            return Err(miette!(
                help = "update hardcoded version",
                "expected two fragments in version"
            ));
        };
        let major = major_str.parse().into_diagnostic()?;
        let minor = minor_str.parse().into_diagnostic()?;
        Ok(Self { major, minor })
    }
}
impl Display for PostgresVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

const PROJECT_FILENAME: &str = "pgd.toml";

/// Configuration stored in pgd.toml
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PGDConfig {
    /// PostgreSQL version to use
    #[serde_as(as = "DisplayFromStr")]
    pub version: PostgresVersion,

    /// Database password
    pub password: String,

    /// Port to bind on host
    pub port: u16,
}

impl PGDConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to read config file: {}", path.display()))?;

        let config: PGDConfig = toml::from_str(&content)
            .into_diagnostic()
            .wrap_err("Failed to parse pgd.toml")?;

        Ok(config)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let content = toml::to_string_pretty(self)
            .into_diagnostic()
            .wrap_err("Failed to serialize config")?;

        std::fs::write(path, content)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to write config file: {}", path.display()))?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Project {
    /// Project name (derived from directory name)
    pub name: String,

    /// Path to the project directory containing pgd.toml
    pub path: PathBuf,

    pub config: PGDConfig,
}

impl Project {
    pub fn container_name(&self) -> String {
        let container_name = format!(
            "pgd-{}-{}",
            self.name,
            self.config.version.to_string().replace('.', "_")
        );
        container_name
    }

    /// Load a project from the current directory
    pub fn load() -> Result<Option<Self>> {
        let project_path = get_project_path()?;
        let config_path = project_path.join(PROJECT_FILENAME);

        if !config_path.exists() {
            return Ok(None);
        }

        let config = PGDConfig::load(&config_path)?;
        let name = Self::extract_project_name(&project_path)?;

        Ok(Some(Project {
            name,
            path: project_path,
            config,
        }))
    }

    pub fn new(config: PGDConfig) -> Result<Self> {
        let project_path = get_project_path()?;
        let name = Self::extract_project_name(&project_path)?;

        let this = Self {
            name,
            path: project_path,
            config,
        };

        this.save_config()?;

        Ok(this)
    }

    /// Extract project name from directory path
    fn extract_project_name(path: &Path) -> Result<String> {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .ok_or_else(|| miette::miette!("Failed to extract project name from path"))
    }

    /// Get the path to the pgd.toml file
    pub fn config_path(&self) -> PathBuf {
        self.path.join("pgd.toml")
    }

    /// Save the current configuration
    pub fn save_config(&self) -> Result<()> {
        self.config.save(self.config_path())
    }
}

fn get_project_path() -> Result<PathBuf, miette::Error> {
    let project_path = std::env::current_dir()
        .into_diagnostic()
        .wrap_err("Failed to get current directory")?;
    Ok(project_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_project_name() {
        let path = PathBuf::from("/home/user/my-project");
        let name = Project::extract_project_name(&path).unwrap();
        assert_eq!(name, "my-project");
    }
}
