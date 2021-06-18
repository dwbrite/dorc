mod daemon;
mod proxy;
mod validators;

use structopt::StructOpt;
use dialoguer::{Input, Select, Validator};
use dialoguer::theme::ColorfulTheme;
use std::fmt::{Debug};
use std::path::Path;
use dialoguer::console::{style};
use tokio::sync::mpsc;
use serde_derive::*;

use proxy::Proxy;
use validators::*;
use serde::Serialize;
use std::fs::{create_dir_all, File};
use fs_extra::dir::CopyOptions;

use anyhow::Result;
use std::collections::HashMap;


#[derive(Debug, PartialEq, StructOpt)]
#[structopt(name = "dorc", about = "devin's orchestrator - a stupid deployment utility")]
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
struct Service {
    qualified_name: String,
    working_dir: String, // defaults to /srv/www/<qualified-service-name>
    port: u16,

    on_start: String,
    on_reload: Option<Vec<String>>,
    on_stop: Option<Vec<String>>, // defaults to kill <pid>
}

impl Service {
    pub fn to_systemd_service(&self) -> systemd_unit::Service {
        systemd_unit::Service {
            unit: systemd_unit::Unit {
                name: self.qualified_name.clone(),
                requires: None, // TODO: require dorc
                // source_path: String::from(format!("/etc/dorc/apps/{}.toml", original_name)),
                ..systemd_unit::Unit::default()
            },
            install: systemd_unit::Install {
                wanted_by: Some(vec!["multi-user.target".to_string()]), // launch when networks are up
                ..systemd_unit::Install::default()
            },
            exec: systemd_unit::Exec {
                working_directory: Some(std::path::PathBuf::from(&self.working_dir)), // TODO:
                ..systemd_unit::Exec::default()
            },
            exec_start: Some(vec![self.on_start.clone()]),
            exec_reload: self.on_reload.clone(),
            exec_stop: self.on_stop.clone(),
            .. systemd_unit::Service::default()
        }
    }
}


impl Service {
    fn from_stdin(qualified_name: String) -> Self {
        let working_dir = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Working dir")
            .default(format!("/etc/dorc/service-data/{}", qualified_name))
            .show_default(true)
            .validate_with( LocationValidator)
            .interact_text()
            .unwrap();

        // TODO: print helper text here
        let port_str: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Service address")
            .validate_with(AddressValidator)
            .interact_text()
            .unwrap();

        let port = port_str.parse().unwrap();

        let on_start = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Start command")
            .default(format!("{} -p {}", qualified_name, port))
            .show_default(true)
            .interact_text()
            .unwrap();

        let on_stop: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Stop command")
            .default(format!("killall {}", qualified_name))
            .show_default(true)
            .interact_text()
            .unwrap();

        let on_reload: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Reload command")
            .allow_empty(true)
            .interact_text()
            .unwrap();

        Self {
            qualified_name,
            working_dir,
            port,
            on_start,
            on_reload: Some(vec![on_reload]),
            on_stop: Some(vec![on_stop])
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct App {
    app_name: String,
    release_dir: String,
    release_bin: String,
    listen_port: u16,

    active_service: String,

    subservices: HashMap<String, Service>,
}

impl App {
    fn load(app_name: String) -> Result<App> {
        let toml = std::fs::read_to_string(format!("/etc/dorc/apps/{}.toml", app_name))?;
        let result: App = toml::from_str(&toml)?;
        Ok(result)
    }

    fn save(&self) {
        let toml = toml::to_string(&self).unwrap();
        create_dir_all("/etc/dorc/apps").expect("Could not create /etc/dorc/apps/");
        std::fs::write(format!("/etc/dorc/apps/{}.toml", self.app_name), toml).expect("Could not write to toml file");
    }

    fn reconstruct_subservice(&self, service: &Service) {
        // ignore error
        std::process::Command::new("systemctl").args(&["stop", &service.qualified_name]).output();

        // TODO: clear dir before copy

        fs_extra::dir::copy(&self.release_dir, &service.working_dir, &CopyOptions {
            overwrite: true,
            skip_exist: false,
            buffer_size: 64000,
            copy_inside: true,
            content_only: false,
            depth: 0
        });

        std::fs::copy(self.release_bin.as_str(), format!("/usr/local/bin/{}", &service.qualified_name));

        let sysdservice = service.to_systemd_service();
        std::fs::write(format!("/etc/systemd/system/{}.service", service.qualified_name), sysdservice.to_string());

        std::process::Command::new("systemctl").args(&["start", &service.qualified_name]).output().expect("failed to start");
        std::process::Command::new("systemctl").args(&["enable", &service.qualified_name]).output().expect("failed to enable");
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
        .unwrap().parse().unwrap();

    println!();
    println!("This tool is for {}/{} deployments.", style("blue").blue(), style("green").green());
    println!("Let's configure {}'s sub-services.", style(&app_name).yellow().bold());

    let blue_service_name = format!("blue-{}", app_name);
    println!("{}", style(format!("\nConfiguring '{}'", blue_service_name)).blue().bold());
    let blue_service = Service::from_stdin(blue_service_name.clone());

    let green_service_name = format!("green-{}", app_name);
    println!("{}", style(format!("\nConfiguring '{}'", green_service_name)).green().bold());
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
        app.reconstruct_subservice(&service);
    }

    println!("\nDone! {} has been registered with two services.", style(&app.app_name).yellow().bold());

    println!("Thanks for using dorc!~");
}

#[tokio::main]
async fn main() {
    let opt: Opt = Opt::from_args();

    match opt.subcommand {
        Subcommands::StartDaemon => { daemon::start().await; },
        Subcommands::Register => register(),
    }
}
