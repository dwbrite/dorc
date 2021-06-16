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
    on_reload: Option<String>,
    on_restart: Option<String>, // defaults to stop; start
    on_stop: Option<String>, // defaults to kill <pid>
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
            .default(format!("/usr/local/bin/{}", qualified_name))
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

        let on_restart: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Restart command")
            .allow_empty(true)
            .interact_text()
            .unwrap();


        Self {
            qualified_name,
            working_dir,
            port,
            on_start,
            on_reload: Some(on_reload),
            on_restart: Some(on_restart),
            on_stop: Some(on_stop)
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct App {
    app_name: String,
    release_dir: String,
    release_bin: String,
    // TODO: deploy strategy
    subservices: Vec<Service>,
}



fn register() {
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

    // let strategies = [
    //     "None",
    //     "Blue/Green",
    // ];
    //
    // let _deploy_strategy: usize = Select::with_theme(&ColorfulTheme::default())
    //     .with_prompt("Deployment strategy")
    //     .default(0)
    //     .items(&strategies[..])
    //     .interact()
    //     .unwrap();
    // TODO: match deployment strategies

    let blue_service_name = format!("blue-{}", app_name);
    println!("{}", style(format!("\nConfiguring '{}'", blue_service_name)).blue().bold());
    let blue_service = Service::from_stdin(blue_service_name);

    let green_service_name = format!("green-{}", app_name);
    println!("{}", style(format!("\nConfiguring '{}'", green_service_name)).green().bold());
    let green_service = Service::from_stdin(green_service_name);

    // now that we have our application _defined_,
    // we can save this configuration in /etc/dorc/apps/{app-name}.toml

    let app = App {
        app_name,
        release_dir,
        release_bin,
        subservices: vec![blue_service, green_service]
    };

    // TODO: error instead of crash in all of these

    // save app config
    let toml = toml::to_string(&app).unwrap();
    create_dir_all("/etc/dorc/apps").expect("Could not create /etc/dorc/apps/");
    std::fs::write(format!("/etc/dorc/apps/{}.toml", app.app_name), toml).expect("Could not write to toml file");

    // move release files to relevant subservice locations
    for service in app.subservices {
        fs_extra::dir::copy(&app.release_dir, &service.working_dir, &CopyOptions {
            overwrite: true,
            skip_exist: false,
            buffer_size: 64000,
            copy_inside: true,
            content_only: false,
            depth: 0
        });

        std::fs::copy(app.release_bin.as_str(), format!("/usr/local/bin/{}", &service.qualified_name));
    }

    // TODO: create systemd service files and start the services
}

#[tokio::main]
async fn main() {
    let opt: Opt = Opt::from_args();

    match opt.subcommand {
        Subcommands::StartDaemon => { daemon::start().await; },
        Subcommands::Register => register(),
    }
}
