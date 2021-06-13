use structopt::StructOpt;
use dialoguer::{Input, Select, Validator, Confirm};
use dialoguer::theme::ColorfulTheme;
use std::fmt::{Debug, Display};
use std::path::Path;
use std::convert::TryFrom;
use std::io::Write;
use dialoguer::console::{style, Term};


#[derive(Debug, PartialEq, StructOpt)]
#[structopt(name = "dorc", about = "devin's orchestrator - a stupid deployment tool")]
struct Opt {
    #[structopt(subcommand)]
    subcommand: Subcommands,
}


#[derive(Debug, PartialEq, StructOpt)]
enum Subcommands {
    Register,
}


struct ServiceNameValidator;
impl Validator<String> for ServiceNameValidator {
    type Err = String;

    fn validate(&mut self, s: &String) -> Result<(), Self::Err> {
        if s.starts_with("-") {
            return Err(String::from("Invalid service name. Service names must not start with `-`."));
        }

        if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.'){
            return Err(String::from("Invalid service name. Service names may contain only ascii-alphanumeric characters, '.', and '-'."));
        }

        Ok(())
    }
}

struct LocationValidator;
impl Validator<String> for LocationValidator {
    type Err = String;

    fn validate(&mut self, s: &String) -> Result<(), Self::Err> {


        let path = Path::new(s);

        if !path.parent().unwrap().is_dir() {
            return Err(String::from("Invalid path. Must be a directory."));
        }

        if path.is_relative() {
            return Err(String::from("Invalid path. Must be absolute."));
        }

        Ok(())
    }
}

struct Service {
    qualified_name: String,
    workspace: String, // defaults to /srv/www/<qualified-service-name>
    port: u16,

    on_start: String,
    on_reload: Option<String>,
    on_restart: Option<String>, // defaults to stop; start
    on_stop: Option<String>, // defaults to kill <pid>
}

impl Service {
    fn from_stdin(qualified_name: String) -> Self {

        unimplemented!("oopsie poopsie");
    }
}



fn register() {
    let _service_name: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Service name")
        .validate_with( ServiceNameValidator)
        .interact_text()
        .unwrap();

    let _release_location: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Release location")
        .default(String::from("/var/tmp/") + _service_name.as_str())
        .show_default(true)
        .validate_with( LocationValidator)
        .interact_text()
        .unwrap();

    let strategies = [
        "None",
        "Blue/Green",
    ];

    let _deploy_strategy: usize = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Deployment strategy")
        .default(0)
        .items(&strategies[..])
        .interact()
        .unwrap();

    // TODO: match deployment strategies

    let blue_service_name = format!("blue-{}", _service_name);
    println!("{}", style(format!("Configuring '{}'", blue_service_name)).blue().bold());
    let blue_service = Service::from_stdin(blue_service_name);

    let green_service_name = format!("green-{}", _service_name);
    println!("{}", style(format!("Configuring '{}'", green_service_name)).green().bold());
    let green_service = Service::from_stdin(green_service_name);

}

fn main() {
    let opt: Opt = Opt::from_args();

    match opt.subcommand {
        Subcommands::Register => register()
    }
}
