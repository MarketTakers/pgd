use miette::miette;
use std::str::FromStr;

use bollard::{
    Docker,
    query_parameters::{
        CreateContainerOptions, CreateImageOptions, InspectContainerOptions, ListImagesOptions,
        StartContainerOptions, StopContainerOptions,
    },
    secret::ContainerCreateBody,
};
use indicatif::MultiProgress;
use miette::{Context, IntoDiagnostic, Result};
use tracing::info;

use crate::{
    config::PostgresVersion,
    consts::{DATABASE, USERNAME},
};

mod download;

const DOCKERHUB_POSTGRES: &str = "postgres";
fn format_image(ver: &PostgresVersion) -> String {
    format!("{DOCKERHUB_POSTGRES}:{}", ver)
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

        download::perform_download(multi, download_progress).await?;

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
