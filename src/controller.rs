use miette::miette;

use colored::Colorize;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table, presets::UTF8_FULL};
use futures::TryStreamExt;
use miette::{IntoDiagnostic, Result};

use crate::{
    cli::ConnectionFormat,
    config::{PGDConfig, Project},
    consts::{DATABASE, USERNAME},
    controller::{docker::DockerController, reconciler::Reconciler},
    state::{InstanceState, StateManager},
};

mod docker;
mod utils;

pub mod reconciler;

pub struct Context {
    docker: DockerController,
    project: Option<Project>,
    instance: Option<InstanceState>,
    state: StateManager,
}

impl Context {
    pub fn require_instance(&self) -> Result<&InstanceState> {
        self.instance.as_ref().ok_or(miette!("This command requires instance. Either initiliaze a project, or pass -I with instance name"))
    }

    pub fn require_project(&self) -> Result<&Project> {
        self.project.as_ref().ok_or(miette!(
            "This command requires project. Please, initiliaze a project."
        ))
    }

    pub async fn new(instance_override: Option<String>) -> Result<Self> {
        let project = Project::load()?;
        let state = StateManager::new()?;

        let instance = match (project.as_ref(), instance_override) {
            (None, None) => None,
            // prioritizing provided instance name
            (_, Some(instance)) => state.get(&instance),
            (Some(project), None) => state.get(&project.name),
        };

        Ok(Self {
            docker: DockerController::new().await?,
            project,
            instance,
            state,
        })
    }
}

/// Main CLI command dispatcher
pub struct Controller {
    ctx: Context,
}
impl Controller {
    pub fn new(ctx: Context) -> Self {
        Self { ctx }
    }

    pub async fn logs(&self, follow: bool) -> Result<()> {
        let instance = self.ctx.require_instance()?;

        let mut logs = self
            .ctx
            .docker
            .stream_logs(&instance.container_id, follow)
            .await;

        while let Some(log) = logs.try_next().await? {
            let bytes = log.into_bytes();
            let line = String::from_utf8_lossy(bytes.as_ref());
            print!("{line}");
        }

        Ok(())
    }

    pub async fn show_connection(&self, format: ConnectionFormat) -> Result<()> {
        let project = self.ctx.require_project()?;
        let reconciler = Reconciler { ctx: &self.ctx };

        reconciler.reconcile(project).await?;

        match format {
            ConnectionFormat::Dsn => {
                println!(
                    "postgres://{}:{}@127.0.0.1:{}/{}",
                    USERNAME, project.config.password, project.config.port, DATABASE
                );
            }
            ConnectionFormat::Human => {
                format_conn_human(project);
            }
        }

        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        let instance = self.ctx.require_instance()?;
        let project = self.ctx.require_project()?;

        if self
            .ctx
            .docker
            .is_container_running_by_id(&instance.container_id)
            .await?
        {
            println!("{}", "Container is already running".yellow());
            return Ok(());
        }

        println!("{}", "Starting container...".cyan());
        self.ctx
            .docker
            .start_container_by_id(&instance.container_id)
            .await?;
        println!(
            "{} {} {}",
            "✓".green().bold(),
            "Container".green(),
            project.container_name().yellow()
        );

        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        let instance = self.ctx.require_instance()?;
        let project = self.ctx.require_project()?;

        if !self
            .ctx
            .docker
            .is_container_running_by_id(&instance.container_id)
            .await?
        {
            println!("{}", "Container is not running".yellow());
            return Ok(());
        }

        println!("{}", "Stopping container...".cyan());
        self.ctx
            .docker
            .stop_container(&instance.container_id, 10)
            .await?;
        println!(
            "{} {} {}",
            "✓".green().bold(),
            "Stopped container".green(),
            project.container_name().yellow()
        );

        Ok(())
    }

    pub async fn restart(&self) -> Result<()> {
        let instance = self.ctx.require_instance()?;
        let project = self.ctx.require_project()?;

        println!("{}", "Restarting container...".cyan());
        self.ctx
            .docker
            .restart_container(&instance.container_id, 10)
            .await?;
        println!(
            "{} {} {}",
            "✓".green().bold(),
            "Restarted container".green(),
            project.container_name().yellow()
        );

        Ok(())
    }

    pub async fn destroy(&self, force: bool) -> Result<()> {
        let instance = self.ctx.require_instance()?;
        let project = self.ctx.require_project()?;

        if !force {
            use cliclack::{confirm, outro};
            let confirmed = confirm(
                format!(
                    "Are you sure you want to destroy container '{}'? This will remove the container and all its volumes.",
                    project.container_name()
                ),
            )
            .interact()
            .into_diagnostic()?;

            if !confirmed {
                outro("Operation cancelled".to_string()).into_diagnostic()?;
                return Ok(());
            }
        }

        println!("{}", "Destroying container...".cyan());

        // Stop if running
        if self
            .ctx
            .docker
            .is_container_running_by_id(&instance.container_id)
            .await?
        {
            self.ctx
                .docker
                .stop_container(&instance.container_id, 5)
                .await?;
        }

        // Remove container
        self.ctx
            .docker
            .remove_container(&instance.container_id, true)
            .await?;

        // Remove from state
        self.ctx.state.remove(&project.name);
        self.ctx.state.save()?;

        println!(
            "{} {} {}",
            "✓".green().bold(),
            "Destroyed container".green(),
            project.container_name().yellow()
        );

        Ok(())
    }

    pub async fn wipe(&self, force: bool) -> Result<()> {
        let instance = self.ctx.require_instance()?;
        let project = self.ctx.require_project()?;

        if !force {
            use cliclack::{confirm, outro};
            let confirmed = confirm(
                "Are you sure you want to wipe all database data? This action cannot be undone."
                    .to_string(),
            )
            .interact()
            .into_diagnostic()?;

            if !confirmed {
                outro("Operation cancelled".to_string()).into_diagnostic()?;
                return Ok(());
            }
        }

        let is_running = self
            .ctx
            .docker
            .is_container_running_by_id(&instance.container_id)
            .await?;

        if !is_running {
            println!("{}", "Starting container to wipe data...".cyan());
            self.ctx
                .docker
                .start_container_by_id(&instance.container_id)
                .await?;
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        }

        println!("{}", "Wiping database...".cyan());

        // Drop and recreate database
        let drop_query = format!("DROP DATABASE IF EXISTS {};", DATABASE);
        let drop_cmd = vec!["psql", "-U", USERNAME, "-c", &drop_query];
        self.ctx
            .docker
            .exec_in_container(&instance.container_id, drop_cmd)
            .await?;

        let create_query = format!("CREATE DATABASE {};", DATABASE);
        let create_cmd = vec!["psql", "-U", USERNAME, "-c", &create_query];
        self.ctx
            .docker
            .exec_in_container(&instance.container_id, create_cmd)
            .await?;

        println!(
            "{} {} {}",
            "✓".green().bold(),
            "Wiped database for".green(),
            project.name.yellow()
        );

        Ok(())
    }

    pub async fn status(&self) -> Result<()> {
        let project = self.ctx.require_project()?;

        let mut table = create_ui_table(format!("Status: {}", project.name));

        table.add_row(vec![
            Cell::new("Project").fg(Color::White),
            Cell::new(&project.name).add_attribute(Attribute::Bold),
        ]);

        table.add_row(vec![
            Cell::new("Container Name").fg(Color::White),
            Cell::new(project.container_name()).add_attribute(Attribute::Bold),
        ]);

        match &self.ctx.instance {
            Some(instance) => {
                let exists = self
                    .ctx
                    .docker
                    .container_exists_by_id(&instance.container_id)
                    .await?;

                if !exists {
                    table.add_row(vec![
                        Cell::new("Status").fg(Color::White),
                        Cell::new("Container not found").fg(Color::Red),
                    ]);
                } else {
                    let is_running = self
                        .ctx
                        .docker
                        .is_container_running_by_id(&instance.container_id)
                        .await?;

                    table.add_row(vec![
                        Cell::new("Status").fg(Color::White),
                        if is_running {
                            Cell::new("Running").fg(Color::Green)
                        } else {
                            Cell::new("Stopped").fg(Color::Yellow)
                        },
                    ]);

                    table.add_row(vec![
                        Cell::new("Container ID").fg(Color::White),
                        Cell::new(&instance.container_id[..12]).fg(Color::DarkGrey),
                    ]);

                    table.add_row(vec![
                        Cell::new("PostgreSQL Version").fg(Color::White),
                        Cell::new(instance.postgres_version.to_string())
                            .add_attribute(Attribute::Bold),
                    ]);

                    table.add_row(vec![
                        Cell::new("Port").fg(Color::White),
                        Cell::new(instance.port.to_string()).add_attribute(Attribute::Bold),
                    ]);

                    // Check for drift
                    if instance.postgres_version != project.config.version {
                        table.add_row(vec![
                            Cell::new("⚠ Version Drift").fg(Color::Yellow),
                            Cell::new(format!(
                                "Config: {}, Container: {}",
                                project.config.version, instance.postgres_version
                            ))
                            .fg(Color::Yellow),
                        ]);
                    }

                    if instance.port != project.config.port {
                        table.add_row(vec![
                            Cell::new("⚠ Port Drift").fg(Color::Yellow),
                            Cell::new(format!(
                                "Config: {}, Container: {}",
                                project.config.port, instance.port
                            ))
                            .fg(Color::Yellow),
                        ]);
                    }
                }
            }
            None => {
                table.add_row(vec![
                    Cell::new("Status").fg(Color::White),
                    Cell::new("Not initialized").fg(Color::Yellow),
                ]);
            }
        }

        println!("{}", table);

        Ok(())
    }

    pub async fn init_project(&self) -> Result<()> {
        let reconciler = Reconciler { ctx: &self.ctx };

        if let Some(project) = &self.ctx.project {
            return reconciler.reconcile(project).await;
        }

        println!("{}", "Initializing new pgd project...".cyan());

        let mut versions = self.ctx.docker.available_versions().await?;
        versions.sort();
        let latest_version = versions
            .last()
            .ok_or(miette!("expected to have at least one version"))?;

        let config = PGDConfig {
            version: *latest_version,
            password: utils::generate_password(),
            port: utils::find_available_port(&self.ctx.state)?,
        };
        let project = Project::new(config)?;

        println!(
            "\nCreated pgd.toml in {}\n",
            project.path.display().to_string().bright_white().bold()
        );

        let mut table = create_ui_table("Project Configuration".to_string());
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

        reconciler.reconcile(&project).await?;

        println!(
            "\n{} {}",
            "✓".green().bold(),
            "Project initialized successfully!".green(),
        );

        Ok(())
    }
}

fn format_conn_human(project: &Project) {
    let mut table = create_ui_table("Instance".to_string());
    table.add_row(vec![
        Cell::new("Project").fg(Color::White),
        Cell::new(&project.name).add_attribute(Attribute::Bold),
    ]);
    table.add_row(vec![
        Cell::new("PostgreSQL Version").fg(Color::White),
        Cell::new(project.config.version.to_string()).add_attribute(Attribute::Bold),
    ]);
    table.add_row(vec![
        Cell::new("Host").fg(Color::White),
        Cell::new("127.0.0.1").add_attribute(Attribute::Bold),
    ]);

    table.add_row(vec![
        Cell::new("Port").fg(Color::White),
        Cell::new(project.config.port.to_string()).add_attribute(Attribute::Bold),
    ]);
    table.add_row(vec![
        Cell::new("Username").fg(Color::White),
        Cell::new(USERNAME).add_attribute(Attribute::Bold),
    ]);

    table.add_row(vec![
        Cell::new("Password").fg(Color::White),
        Cell::new(project.config.password.clone()).fg(Color::DarkGrey),
    ]);
    println!("{}", table);
}

fn create_ui_table(header: String) -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_style(comfy_table::TableComponent::MiddleIntersections, ' ')
        .set_header(vec![Cell::new(header).add_attribute(Attribute::Bold)]);

    use comfy_table::TableComponent::*;
    table.set_style(TopLeftCorner, '╭');
    table.set_style(TopRightCorner, '╮');
    table.set_style(BottomLeftCorner, '╰');
    table.set_style(BottomRightCorner, '╯');
    table
}
