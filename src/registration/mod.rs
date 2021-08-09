use dialoguer::Input;
use dialoguer::theme::ColorfulTheme;

use crate::registration::types::Service;
use crate::registration::validators::{LocationValidator, AddressValidator};

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
