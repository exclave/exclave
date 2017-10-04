extern crate systemd_parser;
extern crate runny;
extern crate regex;

use std::path::Path;
use std::time::Duration;
use std::io::Read;
use std::fs::File;

use self::regex::Regex;
use self::systemd_parser::items::DirectiveEntry;
use self::runny::Runny;
use config::Config;
use unit::{UnitName, UnitSelectError, UnitActivateError, UnitDeactivateError,
           UnitIncompatibleReason, UnitDescriptionError};

pub struct Scenario {
    name: UnitName,
}

/// A struct defining an in-memory representation of a .scenario file
pub struct ScenarioDescription {
    /// The id of the unit (including the kind)
    id: UnitName,

    /// A short name
    name: String,

    /// A detailed description of this jig, up to one paragraph.
    description: String,

    /// A Vec<String> of jig names that this test is compatible with.
    jigs: Vec<String>,

    /// A Vec<String> of test names that are explicitly specified.
    tests: Vec<String>,

    /// A Vec<String> of tests that are considered to have passed without running them.
    assumptions: Vec<String>,

    /// The maximum duration, if any, for this scenario
    timeout: Option<Duration>,

    /// A command to run when a scenario completes successfully.
    exec_stop_success: Option<String>,

    /// The maximum amount of time to allow the "success" script to run for.
    exec_stop_success_timeout: Option<Duration>,

    /// An optional command to run when the scenario does not complete successfully.
    exec_stop_failure: Option<String>,

    /// The maximum amount of time to allow the "failure" script to run for.
    exec_stop_failure_timeout: Option<Duration>,
}

impl ScenarioDescription {
    pub fn from_path(path: &Path) -> Result<ScenarioDescription, UnitDescriptionError> {
        let unit_name = UnitName::from_path(path)?;

        // Parse the file into a systemd unit_file object
        let mut contents = String::with_capacity(8192);
        File::open(path)?.read_to_string(&mut contents)?;
        let unit_file = systemd_parser::parse_string(&contents)?;

        if !unit_file.has_category("Scenario") {
            return Err(UnitDescriptionError::MissingSection("Scenario".to_owned()));
        }

        let mut scenario_description = ScenarioDescription {
            id: unit_name,
            name: "".to_owned(),
            description: "".to_owned(),

            jigs: vec![],
            tests: vec![],
            assumptions: vec![],

            timeout: None,

            exec_stop_success: None,
            exec_stop_success_timeout: None,
            exec_stop_failure: None,
            exec_stop_failure_timeout: None,
        };

        for entry in unit_file.lookup_by_category("Scenario") {
            match entry {
                &DirectiveEntry::Solo(ref directive) => {
                    match directive.key() {
                        "Name" => {
                            scenario_description.name = directive.value().unwrap_or("").to_owned()
                        }
                        "Description" => {
                            scenario_description.description =
                                directive.value().unwrap_or("").to_owned()
                        }
                        "Jigs" => {
                            scenario_description.jigs = match directive.value() {
                                Some(s) => {
                                    s.split(|c| c == ',' || c == ' ')
                                        .map(|s| s.to_string())
                                        .collect()
                                }
                                None => vec![],
                            }
                        }
                        "Tests" => {
                            scenario_description.jigs = match directive.value() {
                                Some(s) => {
                                    s.split(|c| c == ',' || c == ' ')
                                        .map(|s| s.to_string())
                                        .collect()
                                }
                                None => vec![],
                            }
                        }
                        "Assumptions" => {
                            scenario_description.jigs = match directive.value() {
                                Some(s) => {
                                    s.split(|c| c == ',' || c == ' ')
                                        .map(|s| s.to_string())
                                        .collect()
                                }
                                None => vec![],
                            }
                        }
                        &_ => (),
                    }
                }
                &_ => (),
            }
        }
        Ok(scenario_description)
    }

    pub fn id(&self) -> &UnitName {
        &self.id
    }

    /// Determine if a unit is compatible with this system.
    /// Returns Ok(()) if it is, and Err(String) if not.
    pub fn is_compatible(&self, config: &Config) -> Result<(), UnitIncompatibleReason> {
        // If this Jig has a file-existence test, run it.
        // if let Some(ref test_file) = self.test_file {
        // if !Path::new(&test_file).exists() {
        // return Err(UnitIncompatibleReason::TestFileNotPresent(test_file.clone()));
        // }
        // }
        //
        // If this Jig has a test-program, run that program and check the output.
        // if let Some(ref cmd_str) = self.test_program {
        // use std::io::{BufRead, BufReader};
        //
        // let running = Runny::new(cmd_str).directory(&Some(config.working_directory().clone()))
        // .timeout(config.timeout().clone())
        // .path(config.paths().clone())
        // .start()?;
        //
        // let mut reader = BufReader::new(running);
        // let mut buf = String::new();
        // loop {
        // if let Err(_) = reader.read_line(&mut buf) {
        // break;
        // }
        // }
        // let result = reader.get_ref().result();
        // if result != 0 {
        // return Err(UnitIncompatibleReason::TestProgramReturnedNonzero(result, buf));
        // }
        // }
        //
        Ok(())
    }

    pub fn select(&self) -> Result<Scenario, UnitSelectError> {
        Scenario::new(self)
    }
}

impl Scenario {
    pub fn new(desc: &ScenarioDescription) -> Result<Scenario, UnitSelectError> {
        Ok(Scenario { name: desc.id.clone() })
    }

    pub fn activate(&self) -> Result<(), UnitActivateError> {
        Ok(())
    }

    pub fn deactivate(&self) -> Result<(), UnitDeactivateError> {
        Ok(())
    }
}

impl Drop for Scenario {
    fn drop(&mut self) {
        println!("Dropping scenario {}", self.name);
    }
}