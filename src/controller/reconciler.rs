use std::time::Duration;

use miette::{Diagnostic, bail};

use colored::Colorize;
use miette::Result;
use thiserror::Error;
use tracing::info;

use crate::{
    config::{PostgresVersion, Project},
    controller::{
        Context,
        docker::{self},
    },
    state::InstanceState,
};

const MAX_RETRIES: usize = 10;
const VERIFY_DURATION_SECS: u64 = 5;

#[derive(Error, Debug, Diagnostic)]
#[error("Failed to sync container state")]
#[diagnostic(code(pgd::reconcile))]
pub enum ReconcileError {
    AlreadyRunning,
    ImageDownload(#[source] docker::Error),
}

pub struct Reconciler<'a> {
    pub ctx: &'a Context,
}

impl<'a> Reconciler<'a> {
    pub async fn reconcile(&self, project: &Project) -> Result<()> {
        self.ctx
            .docker
            .ensure_version_downloaded(&project.config.version)
            .await?;

        self.ensure_container_running(project).await?;

        Ok(())
    }

    async fn ensure_container_running(&self, project: &Project) -> Result<()> {
        let container_id = match &self.ctx.instance {
            Some(instance) => match self.ensure_container_exists(instance).await? {
                Some(id) => id,
                None => self.update_project_container(project).await?,
            },
            None => self.update_project_container(project).await?,
        };

        let container_version = self
            .ctx
            .docker
            .get_container_postgres_version(&container_id)
            .await?;

        self.ensure_matches_project_version(project, &container_id, container_version)
            .await?;

        if self
            .ctx
            .docker
            .is_container_running_by_id(&container_id)
            .await?
        {
            info!("Container is already running");
            return Ok(());
        }

        use indicatif::{ProgressBar, ProgressStyle};

        let spinner = ProgressBar::new_spinner();
        spinner.enable_steady_tick(Duration::from_millis(100));
        spinner.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        spinner.set_message("Starting container...");

        for attempt in 1..=MAX_RETRIES {
            spinner.set_message(format!(
                "Starting container (attempt {}/{})",
                attempt, MAX_RETRIES
            ));

            let result = self.try_starting_container(&container_id, &spinner).await;

            match result {
                Ok(_) => {
                    spinner.finish_with_message(format!(
                        "{}",
                        "Container started successfully".green().bold()
                    ));
                    return Ok(());
                }
                Err(err) => {
                    spinner.set_message(format!(
                        "{} {}/{} failed: {}",
                        "Attempt".yellow(),
                        attempt,
                        MAX_RETRIES,
                        err
                    ));
                }
            }

            if attempt < MAX_RETRIES {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }

        spinner.finish_with_message(format!("{}", "Failed to start container".red()));
        miette::bail!("Failed to start container after {} attempts", MAX_RETRIES)
    }

    async fn try_starting_container(
        &self,
        container_id: &str,
        spinner: &indicatif::ProgressBar,
    ) -> Result<(), miette::Error> {
        match self.ctx.docker.start_container_by_id(container_id).await {
            Ok(_) => {
                spinner.set_message(format!(
                    "{} ({}s)...",
                    "Verifying container is running".cyan(),
                    VERIFY_DURATION_SECS
                ));

                for i in 0..VERIFY_DURATION_SECS {
                    spinner.set_message(format!(
                        "{} ({}/{}s)",
                        "Verifying container stability".cyan(),
                        i + 1,
                        VERIFY_DURATION_SECS
                    ));
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }

                if self
                    .ctx
                    .docker
                    .is_container_running_by_id(container_id)
                    .await?
                {
                    Ok(())
                } else {
                    miette::bail!("Container stopped unexpectedly after start");
                }
            }
            Err(e) => {
                miette::bail!("Failed to start: {}", e);
            }
        }
    }

    async fn update_project_container(&self, project: &Project) -> Result<String, miette::Error> {
        info!(
            "{} {}",
            "Creating container".cyan(),
            project.container_name().yellow()
        );
        let id = self
            .ctx
            .docker
            .create_postgres_container(
                &project.container_name(),
                &project.config.version,
                &project.config.password,
                project.config.port,
            )
            .await?;
        info!("{}", "Container created successfully".green());
        self.ctx.state.upsert(
            project.name.clone(),
            crate::state::InstanceState::new(
                id.clone(),
                project.config.version,
                project.config.port,
            ),
        );
        self.ctx.state.save()?;
        Ok(id)
    }

    async fn ensure_container_exists(
        &self,
        instance: &InstanceState,
    ) -> Result<Option<String>, miette::Error> {
        let mut container_id = None;
        let id = &instance.container_id;
        if self.ctx.docker.container_exists_by_id(id).await? {
            container_id = Some(id.clone());
        }
        Ok(container_id)
    }

    async fn ensure_matches_project_version(
        &self,
        project: &Project,
        _container_id: &String,
        container_version: PostgresVersion,
    ) -> Result<(), miette::Error> {
        let _: () = if container_version != project.config.version {
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
        };
        Ok(())
    }
}
