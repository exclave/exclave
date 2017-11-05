extern crate dependy;
extern crate humantime;
extern crate regex;
extern crate systemd_parser;

use std::path::{Path, PathBuf};
use std::time::Duration;
use std::io::Read;
use std::fs::File;

use self::dependy::Dependency;
use self::humantime::parse_duration;
use self::regex::Regex;
use self::systemd_parser::items::DirectiveEntry;

use config::Config;
use unit::{UnitName, UnitActivateError, UnitDeactivateError, UnitSelectError, UnitDeselectError,
           UnitIncompatibleReason, UnitDescriptionError};
use unitmanager::UnitManager;

#[derive(Debug, PartialEq)]
enum TestType {
    Simple,
    Daemon,
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
    jigs: Vec<UnitName>,

    /// A Vec<String> of test names that must successfully complete for this test to run.
    requires: Vec<UnitName>,

    /// A Vec<String> of test names that should be attempted first, though this test will still
    /// run even if they fail.
    suggests: Vec<UnitName>,

    /// A Vec<String> of tests that this test implies.  This can be used for debugging tests
    /// that "fake" out things like OTP fuse blowing or device reformatting that you normally
    /// want to skip when fixing things in the factory.
    provides: Vec<UnitName>,

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
    exec_start: PathBuf,

    /// ExecStopFail: When stopping tests, if the test failed, then this stop command will be run.
    exec_stop_failure: Option<PathBuf>,

    /// ExecStopSuccess: When stopping tests, if the test succeeded, then this stop command will be run.
    exec_stop_success: Option<PathBuf>,

    /// working_directory: Directory to run progrms from, if any.
    working_directory: Option<PathBuf>,
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

            exec_start: PathBuf::from(""),
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
                                Some(s) => UnitName::from_list(s, "jig")?,
                                None => vec![],
                            }
                        }
                        "Provides" => {
                            test_description.provides = match directive.value() {
                                Some(s) => UnitName::from_list(s, "test")?,
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
                        "WorkingDirectory" => {
                            test_description.working_directory = match directive.value() {
                                None => None,
                                Some(ps) => Some(PathBuf::from(ps)),
                            }
                        }
                        "ExecStart" => {
                            test_description.exec_start = match directive.value() {
                                None => return Err(UnitDescriptionError::MissingValue("Test".to_owned(), "ExecStart".to_owned())),
                                Some(s) => PathBuf::from(s),
                            }
                        }
                        "Timeout" => {
                            test_description.timeout = match directive.value() {
                                None => None,
                                Some(s) => Some(parse_duration(s)?),
                            }
                        }
                        "ExecStopSuccess" => {
                            test_description.exec_stop_success = match directive.value() {
                                None => None,
                                Some(s) => Some(PathBuf::from(s)),
                            }
                        }
                        "ExecStopSuccessTimeout" => {
                            test_description.exec_stop_success_timeout = match directive.value() {
                                None => None,
                                Some(s) => Some(parse_duration(s)?),
                            }
                        }
                        "ExecStopFailure" => {
                            test_description.exec_stop_failure = match directive.value() {
                                None => None,
                                Some(s) => Some(PathBuf::from(s)),
                            }
                        }
                        "ExecStopFailureTimeout" => {
                            test_description.exec_stop_failure_timeout = match directive.value() {
                                None => None,
                                Some(s) => Some(parse_duration(s)?),
                            }
                        }                        &_ => (),
                    }
                }
                &_ => (),
            }
        }
        if test_description.exec_start.to_string_lossy() == "" {
            return Err(UnitDescriptionError::MissingValue("Test".to_owned(), "ExecStart".to_owned()));
        }
        Ok(test_description)
    }

    pub fn id(&self) -> &UnitName {
        &self.id
    }

    /// Returns true if this test is supported on the named jig.
    pub fn supports_jig(&self, name: &UnitName) -> bool {
        self.jigs.contains(name)
    }

    /// Determine if a unit is compatible with this system.
    pub fn is_compatible(&self, manager: &UnitManager, _: &Config) -> Result<(), UnitIncompatibleReason> {
        if self.jigs.len() == 0 {
            return Ok(());
        }
        for jig_name in &self.jigs {
            if manager.jig_is_loaded(&jig_name) {
                return Ok(());
            }
        }
        Err(UnitIncompatibleReason::IncompatibleJig)
    }

    pub fn load(&self, 
        manager: &UnitManager,
        config: &Config) -> Result<Test, UnitIncompatibleReason> {
        self.is_compatible(manager, config)?;
        Ok(Test::new(self))
    }
}

pub struct Test {
    id: UnitName,
    name: String,
    description: String,
    requirements: Vec<UnitName>,
    suggestions: Vec<UnitName>,
    provides: Vec<UnitName>,
}

impl Test {
    pub fn new(desc: &TestDescription) -> Test {
        Test {
            id: desc.id.clone(),
            name: desc.name.clone(),
            description: desc.description.clone(),
            requirements: desc.requires.clone(),
            suggestions: desc.suggests.clone(),
            provides: desc.provides.clone(),
         }
    }

    pub fn select(&self) -> Result<(), UnitSelectError> {
        Ok(())
    }

    pub fn deselect(&self) -> Result<(), UnitDeselectError> {
        Ok(())
    }

    pub fn activate(&self) -> Result<(), UnitActivateError> {
        Ok(())
    }

    pub fn deactivate(&self) -> Result<(), UnitDeactivateError> {
        Ok(())
    }

    pub fn id(&self) -> &UnitName {
        &self.id
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn description(&self) -> &String {
        &self.description
    }
}

impl Dependency<UnitName> for Test {
    fn name(&self) -> &UnitName {
        &self.id
    }
    fn requirements(&self) -> &Vec<UnitName> {
        &self.requirements
    }
    fn suggestions(&self) -> &Vec<UnitName> {
        &self.suggestions
    }
    fn provides(&self) -> &Vec<UnitName> {
        &self.provides
    }
}