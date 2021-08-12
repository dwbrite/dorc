#![feature(async_closure)]

use std::collections::HashMap;
use std::fmt::Debug;
use std::fs::create_dir_all;
use std::path::Path;

use anyhow::Result;
use dialoguer::console::style;
use dialoguer::Input;
use dialoguer::theme::ColorfulTheme;
use fs_extra::dir::CopyOptions;
use serde::Serialize;
use serde_derive::*;
use structopt::StructOpt;

use registration::validators::*;
use registration::types::Service;
use std::io;
use fs_extra::error::Error;
use std::io::ErrorKind;
use log::*;

mod registration;
mod daemon;

#[derive(Debug, PartialEq, StructOpt)]
#[structopt(
    name = "dorc",
    about = "devin's orchestrator - a stupid deployment utility"
)]
struct Opt {
    #[structopt(subcommand)]
    subcommand: Subcommands,
}

#[derive(Debug, PartialEq, StructOpt)]
enum Subcommands {
    Register,
    StartDaemon,
}


#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct App {
    app_name: String,
    release_dir: String,
    release_bin: String,
    listen_port: u16,

    active_service: String,

    subservices: HashMap<String, Service>,
}

impl App {
    pub(crate) fn load<P: AsRef<Path>>(path: P) -> Result<App> {
        let toml = std::fs::read_to_string(path)?;
        let result: App = toml::from_str(&toml)?;
        Ok(result)
    }

    pub(crate) fn save(&self) {
        let toml = toml::to_string(&self).unwrap();
        create_dir_all("/etc/dorc/apps").expect("Could not create /etc/dorc/apps/");
        std::fs::write(format!("/etc/dorc/apps/{}.toml", self.app_name), toml)
            .expect("Could not write to toml file");
    }

    fn migrate_service(&self, service: &Service) -> Result<()>{
        std::process::Command::new("systemctl")
            .args(&["stop", &service.qualified_name])
            .output()?;

        // Don't clear working directory here,
        // users of dorc may want to store data in files that are subservice specific

        fs_extra::dir::copy(
            &self.release_dir,
            &service.working_dir,
            &CopyOptions {
                overwrite: true,
                skip_exist: false,
                buffer_size: 64000,
                copy_inside: true,
                content_only: false,
                depth: 0,
            },
        )?;

        std::fs::copy(
            self.release_bin.as_str(),
            format!("/usr/local/bin/{}", &service.qualified_name),
        )?;

        let sysdservice = service.to_systemd_service();
        std::fs::write(
            format!("/etc/systemd/system/{}.service", service.qualified_name),
            sysdservice.to_string(),
        )?;

        std::process::Command::new("systemctl")
            .args(&["start", &service.qualified_name])
            .output()?;
        std::process::Command::new("systemctl")
            .args(&["enable", &service.qualified_name])
            .output()?;

        // TODO: undo changes on failure? and/or fail early?

        Ok(())
    }
}

fn register() {
    sudo::escalate_if_needed().expect("Higher privilege required to write service files.");

    let app_name: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("App name")
        .validate_with(AppNameValidator)
        .interact_text()
        .unwrap();

    let release_dir: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Release location")
        .default(format!("/var/tmp/{}", app_name))
        .show_default(true)
        .validate_with(LocationValidator)
        .interact_text()
        .unwrap();

    let release_bin: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Release executable")
        .validate_with(FileValidator)
        .interact_text()
        .unwrap();

    let listen_port: u16 = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Listen port")
        .validate_with(AddressValidator)
        .interact_text()
        .unwrap()
        .parse()
        .unwrap();

    println!();
    println!(
        "This tool is for {}/{} deployments.",
        style("blue").blue(),
        style("green").green()
    );
    println!(
        "Let's configure {}'s sub-services.",
        style(&app_name).yellow().bold()
    );

    let blue_service_name = format!("blue-{}", app_name);
    println!(
        "{}",
        style(format!("\nConfiguring '{}'", blue_service_name))
            .blue()
            .bold()
    );
    let blue_service = Service::from_stdin(blue_service_name.clone());

    let green_service_name = format!("green-{}", app_name);
    println!(
        "{}",
        style(format!("\nConfiguring '{}'", green_service_name))
            .green()
            .bold()
    );
    let green_service = Service::from_stdin(green_service_name.clone());

    let mut subservices = HashMap::new();
    subservices.insert(green_service_name.clone(), green_service);
    subservices.insert(blue_service_name.clone(), blue_service);

    let app = App {
        app_name,
        release_dir,
        release_bin,
        listen_port,
        active_service: green_service_name.clone(),
        subservices,
    };

    app.save();

    // move release files to relevant subservice locations
    for (_, service) in &app.subservices {
        match app.migrate_service(&service) {
            Ok(_) => {
                info!("successfully migrated files from {} to {} for {}", app.release_dir, service.working_dir, service.qualified_name);
            }
            Err(e) => {
                error!("failed to migrate files for {} | {}", service.qualified_name, e);
            }
        }
    }

    println!(
        "\nDone! {} has been registered with two services.",
        style(&app.app_name).yellow().bold()
    );

    println!("Thanks for using dorc!~");
}

#[tokio::main]
async fn main() {
    let opt: Opt = Opt::from_args();

    configure_logging();

    match opt.subcommand {
        Subcommands::StartDaemon => { daemon::start().await; }
        Subcommands::Register => { register(); }
    }
}

fn configure_logging() {
    let mut fern = fern::Dispatch::new();

    if cfg!(debug_assertions) {
        fern = fern.level(log::LevelFilter::Debug);
    } else {
        fern = fern.level(log::LevelFilter::Info);
    }

    fern.chain(std::io::stdout()).apply().unwrap();
}
