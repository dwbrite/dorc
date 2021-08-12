#![feature(async_closure)]
//! # Devin's Orchestrator (`dorc`) - a stupid deployment utility
//!
//! `dorc` is a tool for deploying simple backend services with a green–blue strategy.
//!
//!
//! ## Requirements, warnings, et al
//!
//! `dorc` as it stands will only function on linux systems that use SystemD.
//! That's pretty much the only requirement.
//!
//!
//! ### Not all software will work with `dorc`!
//!
//! Binaries need to have a way to set which port they listen on (e.g., `./yourbin --port 8081`)
//!
//! You may run into trouble if your software uses filesystem as permanent storage if that data is stored relative to the working directory.
//!
//! That means you should be storing data in an external database, or a bucket, or in some absolute path like `/etc/yourapp/data`.
//!
//! ---
//!
//! If you need more (or different) functionality, use a more mainstream deployment tool like k8s or docker swarm.
//!
//!
//! ## Understanding `dorc`
//!
//! To understand more about `dorc`, we need to talk terminology.
//!
//! An _application_ includes a release directory and a release binary.
//! This is the location you should upload files for your software.
//!
//! Applications also consist of _services_ (also referred to as subservices).
//! A service is a living version of your software.
//! Each application has two subservices: `green-{app}` and `blue-{app}`,
//! only one of which receives traffic at a given moment.
//! That is to say, one is considered _active_ and the other is _inactive_.
//! `dorc` services are also registered as _SystemD services_.
//!
//! The _daemon_ is a background process that:
//! - routes traffic from an application's listen port to the current active service,
//! - watches release files to keep the inactive service up-to-date,
//! - listens for commands to load, update, and remove applications;
//! and reload or swap the active service of an application.
//!
//! ---
//!
//! With that out of the way, let's talk about how everything works in practice.
//!
//! First you should configure your CI/CD workflow to upload a release to your server.
//! You can see how I do that for my website [here](https://github.com/dwbrite/website-rs/blob/master/.github/workflows/dwbrite-com.yml).
//!
//! [Install `dorc`](#installing-dorc) on your server.
//! It will be registered as a _SystemD service_ that starts the daemon on boot.
//! If SystemD neglects to start the daemon on install, just run `systemctl start dorc`.
//!
//! You can run `dorc register` to register an application and its subservices.
//!
//! Once you've uploaded a new version of your software, `dorc` will copy that to the inactive service.
//! Then you can call `dorc switch {app}` to swap which subservice is considered active.
//! If you have any problems, simply call `dorc switch {app}` again to roll-back to the previous version.
//!
//! Happy deploying!
//!
//!
//! ## Installing `dorc`
//!
//! If I've uploaded this to crates.io, you can probably just run `cargo install dorc`.
//! Otherwise, you may need to clone the repo and install `dorc` manually, along with the SystemD service file.
//! (On debian-based distros, I've already set up `cargo-deb` to help with this!)
//!


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
