extern crate runny;
extern crate systemd_parser;

use std::path::{Path, PathBuf};
use std::io::Read;
use std::fs::File;

use config::Config;
use unit::{UnitActivateError, UnitDeactivateError, UnitDescriptionError, UnitIncompatibleReason,
           UnitName, UnitSelectError, UnitDeselectError};
use unitmanager::UnitManager;

use self::systemd_parser::items::DirectiveEntry;
use self::runny::Runny;

/// A struct defining an in-memory representation of a .jig file
#[derive(Clone)]
pub struct JigDescription {
    /// The id of the unit (including the kind)
    id: UnitName,

    /// A short name
    name: String,

    /// A detailed description of this jig, up to one paragraph.
    description: String,

    /// Name of the scenario to run by default, if any
    default_scenario: Option<UnitName>,

    /// The default directory for programs on this jig, if any
    working_directory: Option<PathBuf>,

    /// The path to the unit file,
    unit_directory: PathBuf,

    /// A program to run to determine if this jig is compatible, if any
    test_program: Option<String>,

    /// A file whose existence indicates this jig is compatible
    test_file: Option<String>,
}

impl JigDescription {
    pub fn from_path(path: &Path) -> Result<JigDescription, UnitDescriptionError> {
        let unit_name = UnitName::from_path(path)?;

        // Parse the file into a systemd unit_file object
        let mut contents = String::with_capacity(8192);
        File::open(path)?.read_to_string(&mut contents)?;
        Self::from_string(&contents, unit_name, path)
    }

    pub fn from_string(contents: &str, unit_name: UnitName, path: &Path) -> Result<JigDescription, UnitDescriptionError> {
        let unit_file = systemd_parser::parse_string(&contents)?;

        if !unit_file.has_category("Jig") {
            return Err(UnitDescriptionError::MissingSection("Jig".to_owned()));
        }

        let mut jig_description = JigDescription {
            id: unit_name,
            name: "".to_owned(),
            description: "".to_owned(),
            default_scenario: None,
            working_directory: None,
            unit_directory: path.parent().unwrap().to_owned(),
            test_program: None,
            test_file: None,
        };

        for entry in unit_file.lookup_by_category("Jig") {
            match entry {
                &DirectiveEntry::Solo(ref directive) => match directive.key() {
                    "Name" => jig_description.name = directive.value().unwrap_or("").to_owned(),
                    "Description" => {
                        jig_description.description = directive.value().unwrap_or("").to_owned()
                    }
                    "WorkingDirectory" | "DefaultWorkingDirectory" => {
                        if let Some(wd) = directive.value() {
                            jig_description.working_directory = Some(PathBuf::from(wd));
                        }
                    }
                    "TestFile" => {
                        jig_description.test_file = match directive.value() {
                            Some(s) => Some(s.to_owned()),
                            None => None,
                        }
                    }
                    "DefaultScenario" => {
                        jig_description.default_scenario = match directive.value() {
                            Some(s) => Some(UnitName::from_str(s, "scenario")?),
                            None => None,
                        }
                    }
                    "TestProgram" => {
                        jig_description.test_program = match directive.value() {
                            Some(s) => Some(s.to_owned()),
                            None => None,
                        }
                    }
                    &_ => (),
                },
                &_ => (),
            }
        }
        Ok(jig_description)
    }

    /// Determine if a unit is compatible with this system.
    /// Returns Ok(()) if it is, and Err(String) if not.
    pub fn is_compatible(
        &self,
        _: &UnitManager,
        config: &Config,
    ) -> Result<(), UnitIncompatibleReason> {
        // If this Jig has a file-existence test, run it.
        if let Some(ref test_file) = self.test_file {
            if !Path::new(&test_file).exists() {
                return Err(UnitIncompatibleReason::TestFileNotPresent(
                    test_file.clone(),
                ));
            }
        }

        // If this Jig has a test-program, run that program and check the output.
        if let Some(ref cmd_str) = self.test_program {
            use std::io::{BufRead, BufReader};

            let running = Runny::new(cmd_str)
                .directory(&Some(config.working_directory(&self.unit_directory, &self.working_directory).clone()))
                .timeout(config.timeout().clone())
                .path(config.paths().clone())
                .start()?;

            let mut reader = BufReader::new(running);
            let mut buf = String::new();
            loop {
                if let Err(_) = reader.read_line(&mut buf) {
                    break;
                }
            }
            let result = reader.get_ref().result();
            if result != 0 {
                return Err(UnitIncompatibleReason::TestProgramReturnedNonzero(
                    result,
                    buf,
                ));
            }
        }
        Ok(())
    }

    pub fn id(&self) -> &UnitName {
        &self.id
    }

    pub fn load(
        &self,
        manager: &UnitManager,
        config: &Config,
    ) -> Result<Jig, UnitIncompatibleReason> {
        self.is_compatible(manager, config)?;

        Ok(Jig::new(self))
    }
}

pub struct Jig {
    description: JigDescription,
}

impl Jig {
    pub fn new(desc: &JigDescription) -> Jig {
        Jig {
            description: desc.clone(),
        }
    }

    pub fn id(&self) -> &UnitName {
        &self.description.id
    }

    pub fn name(&self) -> &String {
        &self.description.name
    }

    pub fn description(&self) -> &String {
        &self.description.description
    }

    pub fn default_scenario(&self) -> &Option<UnitName> {
        &self.description.default_scenario
    }

    pub fn select(&self) -> Result<(), UnitSelectError> {
        Ok(())
    }

    pub fn deselect(&self) -> Result<(), UnitDeselectError> {
        Ok(())
    }

    pub fn activate(
        &mut self,
        _manager: &UnitManager,
        config: &Config,
    ) -> Result<(), UnitActivateError> {
        if let Some(ref wd) = self.description.working_directory {
            config.set_jig_working_directory(wd);
        } else {
            config.clear_jig_working_directory();
        }
        Ok(())
    }

    pub fn deactivate(&self) -> Result<(), UnitDeactivateError> {
        Ok(())
    }
}
