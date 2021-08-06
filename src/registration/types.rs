use core::option::Option::{None, Some};
use core::option::Option;
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Service {
    pub(crate) qualified_name: String,
    pub(crate) working_dir: String, // defaults to /srv/www/<qualified-service-name>
    pub(crate) port: u16,

    pub(crate) on_start: String,
    pub(crate) on_reload: Option<Vec<String>>,
    pub(crate) on_stop: Option<Vec<String>>, // defaults to kill <pid>
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
            ..systemd_unit::Service::default()
        }
    }
}