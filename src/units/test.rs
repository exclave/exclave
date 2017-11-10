extern crate dependy;
extern crate humantime;
extern crate regex;
extern crate runny;
extern crate systemd_parser;

use std::cell::RefCell;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use self::dependy::Dependency;
use self::humantime::{parse_duration, DurationError};
use self::regex::Regex;
use self::runny::Runny;
use self::runny::running::Running;
use self::systemd_parser::items::DirectiveEntry;

use config::Config;
use unit::{UnitName, UnitActivateError, UnitDeactivateError, UnitSelectError, UnitDeselectError,
           UnitIncompatibleReason, UnitDescriptionError};
use unitmanager::{ManagerControlMessage, ManagerControlMessageContents,
                  UnitManager};

#[derive(Debug, PartialEq, Clone)]
enum TestType {
    Simple,
    Daemon,
}

/// A struct defining an in-memory representation of a .test file
#[derive(Clone)]
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
    exec_start: String,

    /// ExecStopFail: When stopping tests, if the test failed, then this stop command will be run.
    exec_stop_failure: Option<String>,

    /// ExecStopSuccess: When stopping tests, if the test succeeded, then this stop command will be run.
    exec_stop_success: Option<String>,

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
                                Some(s) => s.to_owned(),
                            }
                        }
                        "Timeout" => {
                            test_description.timeout = match directive.value() {
                                None => None,
                                Some(s) => Some(Self::parse_time(s)?),
                            }
                        }
                        "ExecStopSuccess" => {
                            test_description.exec_stop_success = match directive.value() {
                                None => None,
                                Some(s) => Some(s.to_owned()),
                            }
                        }
                        "ExecStopSuccessTimeout" => {
                            test_description.exec_stop_success_timeout = match directive.value() {
                                None => None,
                                Some(s) => Some(Self::parse_time(s)?),
                            }
                        }
                        "ExecStopFailure" => {
                            test_description.exec_stop_failure = match directive.value() {
                                None => None,
                                Some(s) => Some(s.to_owned()),
                            }
                        }
                        "ExecStopFailureTimeout" => {
                            test_description.exec_stop_failure_timeout = match directive.value() {
                                None => None,
                                Some(s) => Some(Self::parse_time(s)?),
                            }
                        }                        &_ => (),
                    }
                }
                &_ => (),
            }
        }
        if test_description.exec_start == "" {
            return Err(UnitDescriptionError::MissingValue("Test".to_owned(), "ExecStart".to_owned()));
        }
        Ok(test_description)
    }

    fn parse_time(time_str: &str) -> Result<Duration, DurationError> {
        if let Ok(val) = time_str.parse::<u64>() {
            Ok(Duration::from_secs(val))
        } else {
            parse_duration(time_str)
        }
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
    description: TestDescription,
    program: Rc<RefCell<Option<Running>>>,
}

impl Test {
    pub fn new(desc: &TestDescription) -> Test {
        Test {
            description: desc.clone(),
            program: Rc::new(RefCell::new(None)),
         }
    }

    pub fn select(&self) -> Result<(), UnitSelectError> {
        Ok(())
    }

    pub fn deselect(&self) -> Result<(), UnitDeselectError> {
        Ok(())
    }

    pub fn activate(
        &mut self,
        manager: &UnitManager,
        config: &Config,
    ) -> Result<(), UnitActivateError> {

        // We'll communicate to the manager through this pipe.
        let ctrl = manager.get_control_channel();

        let cmd = &self.description.exec_start;
        let timeout = &self.description.timeout;

        let mut cmd = Runny::new(cmd);
        if let Some(timeout) = *timeout {
            cmd.timeout(timeout);
        }
        cmd.directory(&Some(config.working_directory(&self.description.working_directory)));
        let mut running = cmd.start()?;

        // Keep track of the last line, which we can use to report test status.
        let last_line = Arc::new(Mutex::new("".to_owned()));

        if self.description.test_type == TestType::Daemon {
            
        }

        self.log_output(&ctrl, &mut running, &last_line);

        // Keep a waiter around in a separate thread to send that AdvanceScenario message upon completion.
        let thr_waiter = running.waiter();
        let thr_control = ctrl.clone();
        let id = self.id().clone();
        thread::spawn(move || {
            thr_waiter.wait();

            thr_control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::TestFinished(thr_waiter.result(), last_line.lock().unwrap().clone()))).ok();
            thr_control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::AdvanceScenario(thr_waiter.result()))).ok();
        });

        *self.program.borrow_mut() = Some(running);

        Ok(())
    }

    pub fn deactivate(&self) -> Result<(), UnitDeactivateError> {
        if let Some(ref running) = *self.program.borrow_mut() {
            running.terminate(None).ok();
        }
        Ok(())
    }

    /// is_daemon() can be used to determine if a test should be stopped
    /// now, or when the scenario is finished.
    pub fn is_daemon(&self) -> bool {
        self.description.test_type == TestType::Daemon
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

    pub fn timeout(&self) -> &Option<Duration> {
        &self.description.timeout
    }

    fn log_output(&self, control: &Sender<ManagerControlMessage>, process: &mut Running, last_line: &Arc<Mutex<String>>) {
        
        let stdout = process.take_output();
        let thr_control = control.clone();
        let thr_last_line = last_line.clone();
        let id = self.id().clone();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let line = line.expect("Unable to get next line");
                *thr_last_line.lock().unwrap() = line.clone();
                if let Err(_) = thr_control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::Log(line))) {
                    break;
                }
            }
        });

        let stderr = process.take_error();
        let thr_control = control.clone();
        let thr_last_line = last_line.clone();
        let id = self.id().clone();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let line = line.expect("Unable to get next line");
                *thr_last_line.lock().unwrap() = line.clone();
                if let Err(_) = thr_control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::LogError(line))) {
                    break;
                }
            }
        });
    }
}

impl Dependency<UnitName> for Test {
    fn name(&self) -> &UnitName {
        &self.description.id
    }
    fn requirements(&self) -> &Vec<UnitName> {
        &self.description.requires
    }
    fn suggestions(&self) -> &Vec<UnitName> {
        &self.description.suggests
    }
    fn provides(&self) -> &Vec<UnitName> {
        &self.description.provides
    }
}