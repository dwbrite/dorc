use dialoguer::Validator;
use std::path::Path;

pub struct AppNameValidator;
impl Validator<String> for AppNameValidator {
    type Err = String;

    fn validate(&mut self, s: &String) -> Result<(), Self::Err> {
        if s.starts_with("-") {
            return Err(String::from("Invalid app name. Service names must not start with `-`."));
        }

        if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.'){
            return Err(String::from("Invalid app name. Service names may contain only ascii-alphanumeric characters, '.', and '-'."));
        }

        Ok(())
    }
}

pub struct LocationValidator;
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

pub struct FileValidator;
impl Validator<String> for FileValidator {
    type Err = String;

    fn validate(&mut self, s: &String) -> Result<(), Self::Err> {
        let path = Path::new(s);

        if !path.is_file() {
            return Err(String::from("Invalid path. Must be a file."));
        }

        if path.is_relative() {
            return Err(String::from("Invalid path. Must be absolute."));
        }

        Ok(())
    }
}

pub struct AddressValidator;
impl Validator<String> for AddressValidator {
    type Err = String;

    fn validate(&mut self, s: &String) -> Result<(), Self::Err> {
        if let Ok(_) = s.parse::<u16>() {
            Ok(())
        } else {
            Err(String::from("Could not parse into address. Only ports are supported."))
        }
    }
}