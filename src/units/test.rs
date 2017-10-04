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

#[derive(Debug, PartialEq)]
enum TestType {
    Simple,
    Daemon,
}

pub struct Test {
    name: UnitName,
}

/// A struct defining an in-memory representation of a .test file
pub struct TestDescription {
    /// The id of the unit (including the kind)
    id: UnitName,

    /// A short name
    name: String,

    /// A detailed description of this jig, up to one paragraph.
    description: String,

    /// A Vec<String> of jig names that this test is compatible with.
    jigs: Vec<String>,

    /// A Vec<String> of test names that must successfully complete for this test to run.
    requires: Vec<String>,

    /// A Vec<String> of test names that should be attempted first, though this test will still
    /// run even if they fail.
    suggests: Vec<String>,

    /// A Vec<String> of tests that this test implies.  This can be used for debugging tests
    /// that "fake" out things like OTP fuse blowing or device reformatting that you normally
    /// want to skip when fixing things in the factory.
    provides: Vec<String>,

    /// The maximum duration this test can be run for.
    timeout: Option<Duration>,

    /// The maximum amount of time to allow an ExecStopSuccess to run.
    exec_stop_success_timeout: Option<Duration>,

    /// The maximum amount of time to allow an ExecStopFailure to run.
    exec_stop_failure_timeout: Option<Duration>,

    /// Type: One of "simple" or "daemon".  For "simple" tests, the return code will indicate pass or fail,
    /// and each line printed will be considered progress.  For "daemon", the process will be forked
    /// and left to run in the background.  See "daemons" in the documentation.
    test_type: TestType,

    /// If present, the daemon won't be considered "ready" until this string is matched.
    test_daemon_ready: Option<Regex>,

    /// ExecStart: The command to run as part of this test.
    exec_start: String,

    /// ExecStopFail: When stopping tests, if the test failed, then this stop command will be run.
    exec_stop_failure: Option<String>,

    /// ExecStopSuccess: When stopping tests, if the test succeeded, then this stop command will be run.
    exec_stop_success: Option<String>,

    /// working_directory: Directory to run progrms from, if any.
    working_directory: Option<String>,
}

impl TestDescription {
    pub fn from_path(path: &Path) -> Result<TestDescription, UnitDescriptionError> {
        let unit_name = UnitName::from_path(path)?;

        // Parse the file into a systemd unit_file object
        let mut contents = String::with_capacity(8192);
        File::open(path)?.read_to_string(&mut contents)?;
        let unit_file = systemd_parser::parse_string(&contents)?;

        if !unit_file.has_category("Test") {
            return Err(UnitDescriptionError::MissingSection("Test".to_owned()));
        }

        let mut test_description = TestDescription {
            id: unit_name,
            name: "".to_owned(),
            description: "".to_owned(),

            jigs: vec![],

            requires: vec![],
            suggests: vec![],
            provides: vec![],

            timeout: None,
            exec_stop_success_timeout: None,
            exec_stop_failure_timeout: None,

            test_type: TestType::Simple,

            test_daemon_ready: None,

            exec_start: "".to_owned(),
            exec_stop_failure: None,
            exec_stop_success: None,
            working_directory: None,
        };

        for entry in unit_file.lookup_by_category("Test") {
            match entry {
                &DirectiveEntry::Solo(ref directive) => {
                    match directive.key() {
                        "Name" => {
                            test_description.name = directive.value().unwrap_or("").to_owned()
                        }
                        "Description" => {
                            test_description.description =
                                directive.value().unwrap_or("").to_owned()
                        }
                        "Jigs" => {
                            test_description.jigs = match directive.value() {
                                Some(s) => {
                                    s.split(|c| c == ',' || c == ' ')
                                        .map(|s| s.to_string())
                                        .collect()
                                }
                                None => vec![],
                            }
                        }
                        "DaemonReadyText" => {
                            test_description.test_daemon_ready = match directive.value() {
                                Some(s) => Some(Regex::new(s)?),
                                None => None,
                            }
                        }

                        "Type" => {
                            test_description.test_type = match directive.value() {
                                Some(s) => {
                                    match s.to_string().to_lowercase().as_ref() {
                                        "simple" => TestType::Simple,
                                        "daemon" => TestType::Daemon,
                                        other => return Err(UnitDescriptionError::InvalidValue(
                                            "Test".to_owned(),
                                        "Type".to_owned(),
                                        other.to_owned(),
                                        vec!["Simple".to_owned(), "Daemon".to_owned()])),
                                    }
                                }
                                None => TestType::Simple,
                            };
                        }
                        &_ => (),
                    }
                }
                &_ => (),
            }
        }
        Ok(test_description)
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

    pub fn select(&self) -> Result<Test, UnitSelectError> {
        Test::new(self)
    }
}

impl Test {
    pub fn new(desc: &TestDescription) -> Result<Test, UnitSelectError> {
        Ok(Test { name: desc.id.clone() })
    }

    pub fn activate(&self) -> Result<(), UnitActivateError> {
        Ok(())
    }

    pub fn deactivate(&self) -> Result<(), UnitDeactivateError> {
        Ok(())
    }
}

impl Drop for Test {
    fn drop(&mut self) {
        println!("Dropping test {}", self.name);
    }
}