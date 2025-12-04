use std::time::Duration;

use miette::{Diagnostic, bail, miette};

use colored::Colorize;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table, presets::UTF8_FULL};
use miette::Result;
use thiserror::Error;

use crate::{
    config::{PGDConfig, PostgresVersion, Project},
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

        reconciler.reconcile(&project).await?;

        println!("\n{}", "✓ Project initialized successfully!".green().bold());

        Ok(())
    }
}
