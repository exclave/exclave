extern crate dependy;
extern crate humantime;
extern crate regex;
extern crate runny;
extern crate systemd_parser;

use std::cell::RefCell;
use std::error::Error;
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
use self::runny::running::{RunningOutput, RunningWaiter};
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

    /// The path to the unit file
    unit_directory: PathBuf,
}

impl TestDescription {
    pub fn from_path(path: &Path) -> Result<TestDescription, UnitDescriptionError> {
        let unit_name = UnitName::from_path(path)?;

        // Parse the file into a systemd unit_file object
        let mut contents = String::with_capacity(8192);
        File::open(path)?.read_to_string(&mut contents)?;
        Self::from_string(&contents, unit_name, path)
    }

    pub fn from_string(contents: &str, unit_name: UnitName, path: &Path) -> Result<TestDescription, UnitDescriptionError> {
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
            unit_directory: path.parent().unwrap().to_owned(),
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
                                Some(s) => {
                                    let mut provides_list = UnitName::from_list(s, "test")?;
                                    provides_list.push(test_description.id.clone());
                                    provides_list
                                },
                                None => vec![test_description.id.clone()],
                            }
                        }
                        "Requires" => {
                            test_description.requires = match directive.value() {
                                Some(s) => UnitName::from_list(s, "test")?,
                                None => vec![],
                            }
                        }
                        "Suggests" => {
                            test_description.suggests = match directive.value() {
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
                            // If a WorkingDirectory was specified, add it to the current directory
                            // (replaces `working_directory` if the new WD is absolute)
                            if let Some(wd) = directive.value() {
                                test_description.working_directory = Some(PathBuf::from(wd));
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

    pub fn load(&self, 
        _manager: &UnitManager,
        _config: &Config) -> Result<Test, UnitIncompatibleReason> {
        Ok(Test::new(self))
    }
}

pub struct Test {
    description: TestDescription,
    program: Rc<RefCell<Option<RunningWaiter>>>,
    result_arc: Arc<Mutex<Option<i32>>>,
    last_line: Arc<Mutex<String>>,
}

impl Test {
    pub fn new(desc: &TestDescription) -> Test {
        Test {
            description: desc.clone(),
            program: Rc::new(RefCell::new(None)),
            result_arc: Arc::new(Mutex::new(None)),
            last_line: Arc::new(Mutex::new("".to_owned())),
         }
    }

    pub fn select(&self, manager: &UnitManager) -> Result<(), UnitSelectError> {
        // If there is at least one jig in the description list, then make sure
        // that jig is loaded.
        if self.description.jigs.len() > 0 {
            let mut compatible = false;
            for jig_name in &self.description.jigs {
                if manager.jig_is_loaded(&jig_name) {
                    compatible = true;
                    break;
                }
            }
            if ! compatible {
                panic!("Incompatible!");
                return Err(UnitSelectError::NoCompatibleJig);
            }
        }

        Ok(())
    }

    pub fn deselect(&self) -> Result<(), UnitDeselectError> {
        Ok(())
    }

    /// Send the "test finished" message and update the local result value.
    /// This ensures that we only send the "Finished" result once.
    pub fn send_finished_once(id: &UnitName,
                              ctrl: &Sender<ManagerControlMessage>,
                              result_val: i32,
                              result_arc: &Arc<Mutex<Option<i32>>>,
                              last_line: &Arc<Mutex<String>>) {

        let mut result = result_arc.lock().unwrap();

        if result.is_none() {
            ctrl.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::TestFinished(result_val, last_line.lock().unwrap().clone()))).ok();
            *result = Some(result_val);
        }
    }

    pub fn activate(
        &mut self,
        manager: &UnitManager,
        config: &Config,
    ) -> Result<(), UnitActivateError> {

        // We'll communicate to the manager through this pipe.
        let ctrl = manager.get_control_channel();
        let id = self.id().clone();

        *self.result_arc.lock().unwrap() = None;

        // Announce to the world that we've started considering this test.
        ctrl.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::TestStarted)).ok();

        let cmd = &self.description.exec_start;
        let timeout = &self.description.timeout;

        let mut cmd = Runny::new(cmd);
        if let Some(timeout) = *timeout {
            cmd.timeout(timeout);
        }
        cmd.directory(&Some(config.working_directory(&self.description.unit_directory, &self.description.working_directory)));
        let mut running = match cmd.start() {
            Ok(r) => r,
            Err(e) => {
                ctrl.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::LogError(format!("unable to start test: {:?}", e)))).unwrap();
                ctrl.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::TestFinished(-3, format!("unable to start test: {:?}", e)))).ok();
                ctrl.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::AdvanceScenario(-3))).ok();
                return Err(UnitActivateError::ExecFailed(e));
            }
        };

        // Keep track of the last line, which we can use to report test status.
        let last_line = self.last_line.clone();

        let waiter = running.waiter();
        let thr_control = ctrl.clone();
        let thr_last_line = self.last_line.clone();
        let thr_result_arc = self.result_arc.clone();
        match self.description.test_type {
            TestType::Daemon => {
                let daemon_ready_string = self.description.test_daemon_ready.clone();

                thread::spawn(move || {
                    Self::log_error(&id, &ctrl, running.take_error(), &last_line);
                    let buf_reader = BufReader::new(running.take_output());
                    let buf_lines = buf_reader.lines();
                    let mut buf_iter = buf_lines.into_iter();
                    if let Some(ref r) = daemon_ready_string {
                        let mut found = false;
                        while let Some(line_result) = buf_iter.next() {
                            match line_result {
                                Err(e) => {
                                    thr_control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::LogError(format!("test daemon raised an error: {}", e.description())))).unwrap();
                                    thr_control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::AdvanceScenario(-2))).ok();
                                    running.terminate(Some(Duration::from_secs(1))).ok();
                                    // thr_control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::TestFinished(-2, thr_last_line.lock().unwrap().clone()))).ok();
                                    Self::send_finished_once(&id, &thr_control, -2, &thr_result_arc, &thr_last_line);
                                    return;
                                }
                                Ok(line) => {
                                    thr_control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::Log(line.clone()))).unwrap();
                                    if r.is_match(&line) {
                                        found = true;
                                        break;
                                    }
                                }
                            }
                        }
                        if !found {
                            thr_control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::LogError(format!("test daemon exited before ready string was found")))).unwrap();
                            thr_control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::AdvanceScenario(-1))).ok();
                            running.terminate(Some(Duration::from_secs(1))).ok();
                            Self::send_finished_once(&id, &thr_control, -1, &thr_result_arc, &thr_last_line);
//                            thr_control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::TestFinished(-1, thr_last_line.lock().unwrap().clone()))).ok();
                            return;
                        }
                    }
                    // Log the output normally, now that the daemon has started up.
                    let thr_thr_control = thr_control.clone();
                    let thr_thr_last_line = last_line.clone();
                    let thr_id = id.clone();
                    thread::spawn(move || {
                        for line in buf_iter {
                            let line = line.expect("Unable to get next line");
                            *thr_thr_last_line.lock().unwrap() = line.clone();
                            if let Err(_) = thr_thr_control.send(ManagerControlMessage::new(&thr_id, ManagerControlMessageContents::Log(line))) {
                                break;
                            }
                        }
                    });

                    // Advance to the next test while this one hangs out.
                    thr_control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::AdvanceScenario(0))).ok();
                    running.wait().ok();
                    Self::send_finished_once(&id, &thr_control, running.result(), &thr_result_arc, &thr_last_line);
                });
            },
            TestType::Simple => {

                // Keep a waiter around in a separate thread to send that AdvanceScenario message upon completion.
                Self::log_output(&id, &ctrl, running.take_output(), &last_line);
                Self::log_error(&id, &ctrl, running.take_error(), &last_line);
                thread::spawn(move || {
                    running.wait().ok();
                    Self::send_finished_once(&id, &thr_control, running.result(), &thr_result_arc, &thr_last_line);
                    thr_control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::AdvanceScenario(running.result()))).ok();
                });
            }
        }
        *self.program.borrow_mut() = Some(waiter);

        Ok(())
    }

    pub fn deactivate(&self, manager: &UnitManager) -> Result<(), UnitDeactivateError> {
        if let Some(ref running) = *self.program.borrow_mut() {
            // For Daemons, if they haven't failed so far, then they might fail when we tell them
            // to quit.  Since they've fulfilled their purpose, issue a "pass" message.
            if self.description.test_type == TestType::Daemon {
                Self::send_finished_once(&self.description.id, &manager.get_control_channel(), 0, &self.result_arc, &self.last_line);
            }
            running.terminate(&None);
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

    fn log_output(id: &UnitName, control: &Sender<ManagerControlMessage>, stdout: RunningOutput, last_line: &Arc<Mutex<String>>) {
        let thr_control = control.clone();
        let thr_last_line = last_line.clone();
        let thr_id = id.clone();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let line = line.expect("Unable to get next line");
                *thr_last_line.lock().unwrap() = line.clone();
                if let Err(_) = thr_control.send(ManagerControlMessage::new(&thr_id, ManagerControlMessageContents::Log(line))) {
                    break;
                }
            }
        });
    }

    fn log_error(id: &UnitName, control: &Sender<ManagerControlMessage>, stderr: RunningOutput, last_line: &Arc<Mutex<String>>) {
        let thr_control = control.clone();
        let thr_last_line = last_line.clone();
        let thr_id = id.clone();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let line = line.expect("Unable to get next line");
                *thr_last_line.lock().unwrap() = line.clone();
                if let Err(_) = thr_control.send(ManagerControlMessage::new(&thr_id, ManagerControlMessageContents::LogError(line))) {
                    break;
                }
            }
        });
    }

    fn provides(&self) -> &Vec<UnitName> {
        &self.description.provides
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