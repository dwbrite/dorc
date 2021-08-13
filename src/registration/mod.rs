use std::collections::HashMap;
use log::{info, error};

use dialoguer::console::style;
use dialoguer::Input;
use dialoguer::theme::ColorfulTheme;

use crate::App;
use crate::registration::types::Service;
use crate::registration::validators::{AddressValidator, AppNameValidator, FileValidator, LocationValidator};

pub(crate) mod types;
pub mod validators;


impl Service {
    pub(crate) fn from_stdin(qualified_name: String) -> Self {
        // TODO: for any inputs using directories impl tab-completion.
        let working_dir = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Working dir")
            .default(format!("/etc/dorc/service-data/{}", qualified_name))
            .show_default(true)
            .validate_with(LocationValidator)
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
            on_stop: Some(vec![on_stop]),
        }
    }
}

pub fn register() {
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
