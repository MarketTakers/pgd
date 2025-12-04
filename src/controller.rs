use std::time::Duration;

use miette::{bail, miette};

use colored::Colorize;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table, presets::UTF8_FULL};
use miette::Result;

use crate::{
    config::{PGDConfig, PostgresVersion, Project},
    controller::docker::DockerController,
    state::{InstanceState, StateManager},
};

mod docker;
mod utils;

const MAX_RETRIES: u32 = 10;
const VERIFY_DURATION_SECS: u64 = 5;

pub struct Controller {
    docker: DockerController,
    project: Option<Project>,
    #[allow(unused)]
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

        println!("{}", "Initializing new pgd project...".cyan());

        let mut versions = self.docker.available_versions().await?;
        versions.sort();
        let latest_version = versions
            .last()
            .ok_or(miette!("expected to have at least one version"))?;

        let config = PGDConfig {
            version: *latest_version,
            password: utils::generate_password(),
            port: utils::find_available_port()?,
        };
        let project = Project::new(config)?;

        println!(
            "\n{} {}\n",
            "Created pgd.toml in",
            project.path.display().to_string().bright_white().bold()
        );

        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_style(comfy_table::TableComponent::MiddleIntersections, ' ')
            .set_header(vec![
                Cell::new("Instance Configuration").add_attribute(Attribute::Bold),
            ]);

        use comfy_table::TableComponent::*;
        table.set_style(TopLeftCorner, '╭');
        table.set_style(TopRightCorner, '╮');
        table.set_style(BottomLeftCorner, '╰');
        table.set_style(BottomRightCorner, '╯');
        table.add_row(vec![
            Cell::new("Project").fg(Color::White),
            Cell::new(&project.name).add_attribute(Attribute::Bold),
        ]);
        table.add_row(vec![
            Cell::new("PostgreSQL Version").fg(Color::White),
            Cell::new(project.config.version.to_string()).add_attribute(Attribute::Bold),
        ]);
        table.add_row(vec![
            Cell::new("Port").fg(Color::White),
            Cell::new(project.config.port.to_string()).add_attribute(Attribute::Bold),
        ]);
        table.add_row(vec![
            Cell::new("Password").fg(Color::White),
            Cell::new("*".repeat(project.config.password.len())).fg(Color::DarkGrey),
        ]);

        println!("{table}");

        self.reconcile(&project).await?;

        println!("\n{}", "✓ Project initialized successfully!".green().bold());

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
            Some(instance) => match self.ensure_container_exists(instance).await? {
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
            println!("{}", "Container is already running".white());
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
        container_id: &String,
        spinner: &indicatif::ProgressBar,
    ) -> Result<(), miette::Error> {
        match self.docker.start_container_by_id(container_id).await {
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

                if self.docker.is_container_running_by_id(container_id).await? {
                    return Ok(());
                } else {
                    miette::bail!("Container stopped unexpectedly after start");
                }
            }
            Err(e) => {
                miette::bail!("Failed to start: {}", e);
            }
        }
    }

    async fn update_project_container(
        &self,
        project: &Project,
        state: &mut StateManager,
    ) -> Result<String, miette::Error> {
        println!(
            "{} {}",
            "Creating container".cyan(),
            project.container_name().yellow()
        );
        let id = self
            .docker
            .create_postgres_container(
                &project.container_name(),
                &project.config.version,
                &project.config.password,
                project.config.port,
            )
            .await?;
        println!("{}", "Container created successfully".green());
        state.set(
            project.name.clone(),
            crate::state::InstanceState::new(
                id.clone(),
                project.config.version,
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
