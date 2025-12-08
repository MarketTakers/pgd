use miette::miette;

use colored::Colorize;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table, presets::UTF8_FULL};
use futures::TryStreamExt;
use miette::Result;

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
            ConnectionFormat::DSN => {
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

        let mut table = create_ui_table("Project Configuration");
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

        println!("\n{}", "✓ Project initialized successfully!".green().bold());

        Ok(())
    }
}

fn format_conn_human(project: &Project) {
    let mut table = create_ui_table("Instance");
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

fn create_ui_table(header: &'static str) -> Table {
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
