use miette::{bail, miette};
use rand::{Rng, distr::Alphanumeric};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Write, pin::pin, str::FromStr};

use bollard::{
    Docker,
    errors::Error,
    query_parameters::{
        CreateContainerOptions, CreateImageOptions, InspectContainerOptions, ListImagesOptions,
        ListImagesOptionsBuilder, SearchImagesOptions, StartContainerOptions, StopContainerOptions,
    },
    secret::{ContainerConfig, ContainerCreateBody, CreateImageInfo},
};
use futures::{Stream, StreamExt, TryStreamExt};
use indicatif::{MultiProgress, ProgressBar, ProgressState, ProgressStyle};
use miette::{Context, IntoDiagnostic, Result, diagnostic};
use tracing::info;

use crate::{
    config::{PgxConfig, PostgresVersion, Project},
    state::{InstanceState, StateManager},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ContainerStatus {
    Running,
    Stopped,
    Paused,
    Restarting,
    Dead,
    Unknown,
}

const DOCKERHUB_POSTGRES: &str = "postgres";
const DEFAULT_POSTGRES_PORT: u16 = 5432;
const PORT_SEARCH_RANGE: u16 = 100;

fn format_image(ver: &PostgresVersion) -> String {
    format!("{DOCKERHUB_POSTGRES}:{}", ver.to_string())
}

fn find_available_port() -> Result<u16> {
    use std::net::TcpListener;

    for port in DEFAULT_POSTGRES_PORT..(DEFAULT_POSTGRES_PORT + PORT_SEARCH_RANGE) {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }

    miette::bail!(
        "No available ports found in range {}-{}",
        DEFAULT_POSTGRES_PORT,
        DEFAULT_POSTGRES_PORT + PORT_SEARCH_RANGE - 1
    )
}

fn new_download_pb(multi: &MultiProgress, layer_id: &str) -> ProgressBar {
    let pb = multi.add(ProgressBar::new(0));
    pb.set_style(
        ProgressStyle::with_template(&format!(
            "{{spinner:.green}} [{{elapsed_precise}}] {{msg}} [{{wide_bar:.cyan/blue}}] {{bytes}}/{{total_bytes}} ({{eta}})"
        ))
        .unwrap()
        .with_key("eta", |state: &ProgressState, w: &mut dyn Write| {
            write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap()
        })
        .progress_chars("#>-"),
    );
    pb.set_message(format!("Layer {}", layer_id));
    pb
}

// sadly type ... = impl ... is unstable
pub async fn perform_download(
    multi: MultiProgress,
    chunks: impl Stream<Item = Result<CreateImageInfo, Error>>,
) -> Result<()> {
    let mut chunks = pin!(chunks);
    let mut layer_progress: HashMap<String, ProgressBar> = HashMap::new();

    while let Some(download_info) = chunks.try_next().await.into_diagnostic()? {
        download_check_for_error(&mut layer_progress, &download_info)?;

        let layer_id = download_info.id.as_deref().unwrap_or("unknown");

        // Get or create progress bar for this layer
        let pb = layer_progress
            .entry(layer_id.to_string())
            .or_insert_with(|| new_download_pb(&multi, layer_id));

        download_drive_progress(pb, download_info);
    }

    // Clean up any remaining progress bars
    for (_, pb) in layer_progress.drain() {
        pb.finish_and_clear();
    }

    Ok(())
}

fn download_drive_progress(pb: &mut ProgressBar, download_info: CreateImageInfo) {
    match download_info.progress_detail {
        Some(info) => match (info.current, info.total) {
            (None, None) => {
                pb.inc(1);
            }
            (current, total) => {
                if let Some(total) = total {
                    pb.set_length(total as u64);
                }
                if let Some(current) = current {
                    pb.set_position(current as u64);
                }

                if let (Some(current), Some(total)) = (current, total)
                    && (current == total)
                {
                    pb.finish_with_message("Completed!");
                }
            }
        },
        None => {
            // No progress detail, just show activity
            pb.tick();
        }
    }
}

fn download_check_for_error(
    layer_progress: &mut HashMap<String, ProgressBar>,
    download_info: &CreateImageInfo,
) -> Result<()> {
    if let Some(error_detail) = &download_info.error_detail {
        for (_, pb) in layer_progress.drain() {
            pb.finish_and_clear();
        }

        match (error_detail.code, &error_detail.message) {
            (None, Some(msg)) => miette::bail!("docker image download error: {}", msg),
            (Some(code), None) => miette::bail!("docker image download error: code {}", code),
            (Some(code), Some(msg)) => {
                miette::bail!(
                    "docker image download error: code {}, message: {}",
                    code,
                    msg
                )
            }
            _ => (),
        }
    }

    Ok(())
}

pub struct DockerController {
    daemon: Docker,
}

impl DockerController {
    pub async fn new() -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()
        .into_diagnostic()
        .wrap_err(
            "Failed to connect to Docker! pgx required Docker installed. Make sure it's running.",
        )?;

        info!("docker.created");

        docker
            .list_images(Some(ListImagesOptions::default()))
            .await
            .into_diagnostic()
            .wrap_err("Docker basic connectivity test refused")?;

        Ok(Self { daemon: docker })
    }

    pub async fn download_image(&self, image: String) -> Result<()> {
        let options = Some(CreateImageOptions {
            from_image: Some(image.clone()),
            ..Default::default()
        });

        let download_progress = self.daemon.create_image(options, None, None);

        let multi = MultiProgress::new();

        println!("Downloading {image}");

        perform_download(multi, download_progress).await?;

        println!("Download complete!");

        Ok(())
    }

    pub async fn ensure_version_downloaded(&self, ver: &PostgresVersion) -> Result<()> {
        let desired_image_tag = format_image(ver);

        let images = self
            .daemon
            .list_images(Some(ListImagesOptions::default()))
            .await
            .into_diagnostic()
            .wrap_err("failed to list installed docker images")?;

        let is_downloaded = images
            .iter()
            .any(|img| img.repo_tags.contains(&desired_image_tag));

        if !is_downloaded {
            self.download_image(desired_image_tag).await?;
        }

        Ok(())
    }

    // TODO: make client to get available versions from dockerhub
    pub async fn available_versions(&self) -> Result<Vec<PostgresVersion>> {
        Ok(vec!["18.1", "17.7", "16.11", "15.15", "14.20"]
            .into_iter()
            .map(|v| PostgresVersion::from_str(v).unwrap())
            .collect())
    }

    pub async fn container_exists(&self, container_id: &str) -> Result<bool> {
        match self
            .daemon
            .inspect_container(container_id, None::<InspectContainerOptions>)
            .await
        {
            Ok(_) => Ok(true),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(false),
            Err(e) => Err(e)
                .into_diagnostic()
                .wrap_err("Failed to inspect container"),
        }
    }

    pub async fn is_container_running(&self, container_name: &str) -> Result<bool> {
        let container = self
            .daemon
            .inspect_container(container_name, None::<InspectContainerOptions>)
            .await
            .into_diagnostic()
            .wrap_err("Failed to inspect container")?;

        Ok(container.state.and_then(|s| s.running).unwrap_or(false))
    }

    pub async fn create_postgres_container(
        &self,
        container_name: &str,
        version: &PostgresVersion,
        password: &str,
        port: u16,
    ) -> Result<String> {
        use bollard::models::{HostConfig, PortBinding};
        use std::collections::HashMap;

        let image = format_image(version);

        let env = vec![
            format!("POSTGRES_PASSWORD={}", password),
            format!("POSTGRES_USER={}", USERNAME),
            format!("POSTGRES_DB={}", DATABASE),
        ];

        let mut port_bindings = HashMap::new();
        port_bindings.insert(
            "5432/tcp".to_string(),
            Some(vec![PortBinding {
                host_ip: Some("127.0.0.1".to_string()),
                host_port: Some(port.to_string()),
            }]),
        );

        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            ..Default::default()
        };

        let mut labels = HashMap::new();
        labels.insert("pgx.postgres.version".to_string(), version.to_string());

        let config = ContainerCreateBody {
            image: Some(image),
            env: Some(env),
            host_config: Some(host_config),
            labels: Some(labels),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: Some(container_name.to_owned()),
            platform: String::new(),
        };

        let response = self
            .daemon
            .create_container(Some(options), config)
            .await
            .into_diagnostic()
            .wrap_err("Failed to create container")?;

        Ok(response.id)
    }

    pub async fn start_container(&self, container_id: &str) -> Result<()> {
        self.daemon
            .start_container(container_id, None::<StartContainerOptions>)
            .await
            .into_diagnostic()
            .wrap_err("Failed to start container")?;

        Ok(())
    }

    pub async fn container_exists_by_id(&self, container_id: &str) -> Result<bool> {
        match self
            .daemon
            .inspect_container(container_id, None::<InspectContainerOptions>)
            .await
        {
            Ok(_) => Ok(true),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(false),
            Err(e) => Err(e)
                .into_diagnostic()
                .wrap_err("Failed to inspect container by ID"),
        }
    }

    pub async fn is_container_running_by_id(&self, container_id: &str) -> Result<bool> {
        let container = self
            .daemon
            .inspect_container(container_id, None::<InspectContainerOptions>)
            .await
            .into_diagnostic()
            .wrap_err("Failed to inspect container")?;

        Ok(container.state.and_then(|s| s.running).unwrap_or(false))
    }

    pub async fn start_container_by_id(&self, container_id: &str) -> Result<()> {
        self.start_container(container_id).await
    }

    pub async fn stop_container(&self, container_id: &str, timeout: i32) -> Result<()> {
        self.daemon
            .stop_container(
                container_id,
                Some(StopContainerOptions {
                    t: Some(timeout),
                    signal: None,
                }),
            )
            .await
            .into_diagnostic()
            .wrap_err("Failed to stop container")?;

        Ok(())
    }

    pub async fn get_container_postgres_version(
        &self,
        container_id: &str,
    ) -> Result<PostgresVersion> {
        let container = self
            .daemon
            .inspect_container(container_id, None::<InspectContainerOptions>)
            .await
            .into_diagnostic()
            .wrap_err("Failed to inspect container")?;

        let labels = container
            .config
            .and_then(|c| c.labels)
            .ok_or_else(|| miette!("Container has no labels"))?;

        let version_str = labels
            .get("pgx.postgres.version")
            .ok_or_else(|| miette!("Container missing pgx.postgres.version label"))?;

        PostgresVersion::from_str(version_str)
            .map_err(|_| miette!("Invalid version in label: {}", version_str))
    }
}

const USERNAME: &str = "postgres";
const DATABASE: &str = "postgres";

const PASSWORD_LENGTH: usize = 16;
pub fn generate_password() -> String {
    let password = (&mut rand::rng())
        .sample_iter(Alphanumeric)
        .take(PASSWORD_LENGTH)
        .map(|b| b as char)
        .collect();
    password
}

const MAX_RETRIES: u32 = 10;
const VERIFY_DURATION_SECS: u64 = 5;

pub struct Controller {
    pub docker: DockerController,
    project: Option<Project>,
    state: StateManager,
}

impl Controller {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            docker: DockerController::new().await?,
            project: Project::load()?,
            state: StateManager::load()?,
        })
    }

    pub async fn init_project(&self) -> Result<()> {
        if let Some(project) = &self.project {
            return self.reconcile(project).await;
        }

        println!("Initializing new pgx project...");

        let mut versions = self.docker.available_versions().await?;
        versions.sort();
        let latest_version = versions
            .last()
            .ok_or(miette!("expected to have at least one version"))?;

        let config = PgxConfig {
            version: *latest_version,
            password: generate_password(),
            port: find_available_port()?,
        };
        let project = Project::new(config)?;

        println!("Created pgx.toml in {}", project.path.display());
        println!("  Project: {}", project.name);
        println!("  PostgreSQL version: {}", project.config.version);
        println!("  Port: {}", project.config.port);
        println!("  Password: {}", "*".repeat(project.config.password.len()));

        self.reconcile(&project).await?;

        println!("\nProject initialized successfully!");

        Ok(())
    }

    pub async fn reconcile(&self, project: &Project) -> Result<()> {
        self.docker
            .ensure_version_downloaded(&project.config.version)
            .await?;

        self.ensure_container_running(project).await?;

        Ok(())
    }

    async fn ensure_container_running(&self, project: &Project) -> Result<()> {
        let mut state = StateManager::load()?;
        let instance_state = state.get_mut(&project.name);

        let container_id = match instance_state {
            Some(instance) => match self.ensure_container_exists(&instance).await? {
                Some(id) => id,
                None => self.update_project_container(project, &mut state).await?,
            },
            None => self.update_project_container(project, &mut state).await?,
        };

        let container_version = self
            .docker
            .get_container_postgres_version(&container_id)
            .await?;

        self.ensure_matches_project_version(project, &mut state, &container_id, container_version)
            .await?;

        if self
            .docker
            .is_container_running_by_id(&container_id)
            .await?
        {
            println!("Container is already running");
            return Ok(());
        }

        println!("Starting container...");

        for attempt in 1..=MAX_RETRIES {
            let result = self.try_starting_container(&container_id, attempt).await;

            match result {
                Ok(_) => break,
                Err(err) => println!("Error: {:#?}", err),
            }

            if attempt < MAX_RETRIES {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }

        miette::bail!("Failed to start container after {} attempts", MAX_RETRIES)
    }

    async fn try_starting_container(
        &self,
        container_id: &String,
        attempt: u32,
    ) -> Result<(), miette::Error> {
        Ok(
            match self.docker.start_container_by_id(container_id).await {
                Ok(_) => {
                    tokio::time::sleep(tokio::time::Duration::from_secs(VERIFY_DURATION_SECS))
                        .await;

                    if self.docker.is_container_running_by_id(container_id).await? {
                        println!("Container started successfully and verified running");

                        return Ok(());
                    } else {
                        println!(
                            "Container stopped unexpectedly after start (attempt {}/{})",
                            attempt, MAX_RETRIES
                        );
                    }
                }
                Err(e) => {
                    println!(
                        "Failed to start container (attempt {}/{}): {}",
                        attempt, MAX_RETRIES, e
                    );
                }
            },
        )
    }

    async fn update_project_container(
        &self,
        project: &Project,
        state: &mut StateManager,
    ) -> Result<String, miette::Error> {
        println!("Creating container {}...", project.container_name());
        let id = self
            .docker
            .create_postgres_container(
                &project.container_name(),
                &project.config.version,
                &project.config.password,
                project.config.port,
            )
            .await?;
        println!("Container created successfully");
        state.set(
            project.name.clone(),
            crate::state::InstanceState::new(
                id.clone(),
                project.config.version.to_string(),
                DATABASE.to_string(),
                USERNAME.to_string(),
                project.config.port,
            ),
        );
        state.save()?;
        Ok(id)
    }

    async fn ensure_container_exists(
        &self,
        instance: &InstanceState,
    ) -> Result<Option<String>, miette::Error> {
        let mut container_id = None;
        let id = &instance.container_id;
        if self.docker.container_exists_by_id(id).await? {
            container_id = Some(id.clone());
        }
        Ok(container_id)
    }

    async fn ensure_matches_project_version(
        &self,
        project: &Project,
        _state: &mut StateManager,
        _container_id: &String,
        container_version: PostgresVersion,
    ) -> Result<(), miette::Error> {
        Ok(if container_version != project.config.version {
            let needs_upgrade = container_version < project.config.version;

            if needs_upgrade {
                bail!("Upgrades are currently unsupported! :(");
                // println!(
                //     "Upgrading PostgreSQL from {} to {}...",
                //     container_version, project.config.version
                // );
                // self.docker.stop_container(container_id, 10).await?;
                // self.docker
                //     .upgrade_container_image(
                //         container_id,
                //         container_name,
                //         &project.config.version,
                //         &project.config.password,
                //         project.config.port,
                //     )
                //     .await?;

                // if let Some(instance_state) = state.get_mut(&project.name) {
                //     instance_state.postgres_version = project.config.version.to_string();
                //     state.save()?;
                // }
            } else {
                miette::bail!(
                    "Cannot downgrade PostgreSQL from {} to {}. Downgrades are not supported.",
                    container_version,
                    project.config.version
                );
            }
        })
    }
}
