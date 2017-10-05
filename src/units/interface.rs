extern crate runny;
extern crate systemd_parser;

use std::path::Path;
use std::io::Read;
use std::fs::File;

use config::Config;
use unit::{UnitActivateError, UnitDeactivateError, UnitDescriptionError, UnitIncompatibleReason,
           UnitName};
use unitlibrary::UnitLibrary;

use self::systemd_parser::items::DirectiveEntry;
use self::runny::Runny;

enum InterfaceFormat {
    Text,
    JSON,
}

/// A struct defining an in-memory representation of a .Interface file
pub struct InterfaceDescription {
    /// The id of the unit (including the kind)
    id: UnitName,

    /// A short name
    name: String,

    /// A detailed description of this Interface, up to one paragraph.
    description: String,

    /// A Vec<String> of jig names that this test is compatible with.
    jigs: Vec<UnitName>,

    /// Path to the command to start the interface
    exec_start: String,

    /// The format expected by the interface
    format: InterfaceFormat,
}

impl InterfaceDescription {
    pub fn from_path(path: &Path) -> Result<InterfaceDescription, UnitDescriptionError> {
        let unit_name = UnitName::from_path(path)?;

        // Parse the file into a systemd unit_file object
        let mut contents = String::with_capacity(8192);
        File::open(path)?.read_to_string(&mut contents)?;
        let unit_file = systemd_parser::parse_string(&contents)?;

        if !unit_file.has_category("Interface") {
            return Err(UnitDescriptionError::MissingSection("Interface".to_owned()));
        }

        let mut interface_description = InterfaceDescription {
            id: unit_name,
            name: "".to_owned(),
            description: "".to_owned(),
            jigs: vec![],
            format: InterfaceFormat::Text,
            exec_start: "".to_owned(),
        };

        for entry in unit_file.lookup_by_category("Interface") {
            match entry {
                &DirectiveEntry::Solo(ref directive) => {
                    match directive.key() {
                        "Name" => {
                            interface_description.name = directive.value().unwrap_or("").to_owned()
                        }
                        "Description" => {
                            interface_description.description =
                                directive.value().unwrap_or("").to_owned()
                        }
                        "Jigs" => {
                            interface_description.jigs = match directive.value() {
                                Some(s) => UnitName::from_list(s, "jig")?,
                                None => vec![],
                            }
                        }
                        "ExecStart" => {
                            interface_description.exec_start =
                                match directive.value() {
                                    Some(s) => s.to_owned(),
                                    None => return Err(UnitDescriptionError::MissingValue("Interface".to_owned(), "ExecStart".to_owned())),
                                }
                        }
                        "Format" => {
                            interface_description.format = match directive.value() {
                                None => InterfaceFormat::Text,
                                Some(s) => {
                                    match s.to_string().to_lowercase().as_ref() {
                                        "text" => InterfaceFormat::Text,
                                        "json" => InterfaceFormat::JSON,
                                        other => return Err(UnitDescriptionError::InvalidValue("Interface".to_owned(), "Format".to_owned(), other.to_owned(), vec!["text".to_owned(), "json".to_owned()])),
                                    }
                                }
                            }
                        }
                        &_ => (),
                    }
                }
                &_ => (),
            }
        }
        Ok(interface_description)
    }

    /// Returns true if this test is supported on the named jig.
    pub fn supports_jig(&self, name: &UnitName) -> bool {
        self.jigs.contains(name)
    }

    /// Determine if a unit is compatible with this system.
    pub fn is_compatible(&self,
                         library: &UnitLibrary,
                         _: &Config)
                         -> Result<(), UnitIncompatibleReason> {
        if self.jigs.len() == 0 {
            return Ok(());
        }
        for jig_name in &self.jigs {
            if library.jig_is_loaded(&jig_name) {
                return Ok(());
            }
        }
        Err(UnitIncompatibleReason::IncompatibleJig)
    }

    pub fn id(&self) -> &UnitName {
        &self.id
    }

    pub fn select(&self,
                  library: &UnitLibrary,
                  config: &Config)
                  -> Result<Interface, UnitIncompatibleReason> {
        self.is_compatible(library, config)?;

        Ok(Interface::new(self))
    }
}

pub struct Interface {
    name: UnitName,
}

impl Interface {
    pub fn new(desc: &InterfaceDescription) -> Interface {
        Interface { name: desc.id.clone() }
    }

    pub fn name(&self) -> &UnitName {
        &self.name
    }

    pub fn activate(&self) -> Result<(), UnitActivateError> {
        Ok(())
    }

    pub fn deactivate(&self) -> Result<(), UnitDeactivateError> {
        Ok(())
    }
}