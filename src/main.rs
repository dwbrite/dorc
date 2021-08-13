#![feature(async_closure)]

use std::collections::HashMap;
use std::fmt::Debug;
use std::fs::create_dir_all;
use std::{fs};
use std::path::Path;

use anyhow::Result;
use fs_extra::dir::CopyOptions;
use serde::Serialize;
use serde_derive::*;
use structopt::StructOpt;

use registration::types::Service;

mod registration;
mod daemon;

const SERVICE_FILE_PATH: &str = "/usr/lib/systemd/system/dorc.service";

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

fn check_install() {
    if Path::new(SERVICE_FILE_PATH).exists() {
        return;
    }

    sudo::escalate_if_needed().expect("Higher privilege required to write service files.");

    let service = include_str!("../meta/debian/dorc.service");
    fs::write(SERVICE_FILE_PATH, service).unwrap();
}

#[tokio::main]
async fn main() {
    let opt: Opt = Opt::from_args();

    configure_logging();

    match opt.subcommand {
        Subcommands::StartDaemon => { check_install(); daemon::start().await; }
        Subcommands::Register => { check_install(); registration::register(); }
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
