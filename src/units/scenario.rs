extern crate dependy;
extern crate humantime;
extern crate runny;
extern crate systemd_parser;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::sync::mpsc::Sender;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

use self::dependy::{Dependy, Dependency};
use self::humantime::{parse_duration, DurationError};
use self::runny::Runny;
use self::runny::running::Running;
use self::systemd_parser::items::DirectiveEntry;

use config::Config;
use unit::{UnitActivateError, UnitDeactivateError, UnitDescriptionError, UnitIncompatibleReason,
           UnitName, UnitSelectError, UnitDeselectError};
use unitmanager::{ManagerControlMessage, ManagerControlMessageContents,
                  UnitManager};
use units::test::{Test, TestState};

struct AssumptionDependency {
    name: UnitName,
    requirements: Vec<UnitName>,
    suggestions: Vec<UnitName>,
    provides: Vec<UnitName>,
}

impl AssumptionDependency {
    pub fn new(name: UnitName) -> AssumptionDependency {
        AssumptionDependency {
            name: name,
            requirements: vec![],
            suggestions: vec![],
            provides: vec![],
        }
    }
}

impl Dependency<UnitName> for AssumptionDependency {
    fn name(&self) -> &UnitName {
        &self.name
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

/// A struct defining an in-memory representation of a .scenario file
#[derive(Clone)]
pub struct ScenarioDescription {
    /// The id of the unit (including the kind)
    id: UnitName,

    /// A short name
    name: String,

    /// A detailed description of this jig, up to one paragraph.
    description: String,

    /// A Vec<String> of jig names that this test is compatible with.
    jigs: Vec<UnitName>,

    /// A Vec<String> of test names that are explicitly specified.
    tests: Vec<UnitName>,

    /// A Vec<String> of tests that are considered to have passed without running them.
    assumptions: Vec<UnitName>,

    /// The maximum duration, if any, for this scenario
    timeout: Option<Duration>,

    /// A default working directory to start from.  Overrides Jig and global config paths.
    working_directory: Option<PathBuf>,

    /// A preflight command to run before the scenario starts.  A failure here will prevent the test from running.
    exec_start: Option<String>,

    /// The maximum amount of time to allow the "start" script to run for.
    exec_start_timeout: Option<Duration>,

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

            working_directory: None,

            exec_start: None,
            exec_start_timeout: None,
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
                                Some(s) => UnitName::from_list(s, "jig")?,
                                None => vec![],
                            }
                        }
                        "WorkingDirectory" => {
                            scenario_description.working_directory = match directive.value() {
                                None => None,
                                Some(ps) => Some(PathBuf::from(ps)),
                            }
                        }
                        "Tests" => {
                            scenario_description.tests = match directive.value() {
                                Some(s) => UnitName::from_list(s, "test")?,
                                None => vec![],
                            }
                        }
                        "Assume" => {
                            scenario_description.assumptions = match directive.value() {
                                Some(s) => UnitName::from_list(s, "test")?,
                                None => vec![],
                            }
                        }
                        "ExecStart" => {
                            scenario_description.exec_start = match directive.value() {
                                None => None,
                                Some(s) => Some(s.to_owned()),
                            }
                        }
                        "ExecStartTimeout" => {
                            scenario_description.exec_start_timeout = match directive.value() {
                                None => None,
                                Some(s) => Some(Self::parse_time(s)?),
                            }
                        }
                        "Timeout" => {
                            scenario_description.timeout = match directive.value() {
                                None => None,
                                Some(s) => Some(Self::parse_time(s)?),
                            }
                        }
                        "ExecStopSuccess" => {
                            scenario_description.exec_stop_success = match directive.value() {
                                None => None,
                                Some(s) => Some(s.to_owned()),
                            }
                        }
                        "ExecStopSuccessTimeout" => {
                            scenario_description.exec_stop_success_timeout = match directive.value() {
                                None => None,
                                Some(s) => Some(Self::parse_time(s)?),
                            }
                        }
                        "ExecStopFailure" => {
                            scenario_description.exec_stop_failure = match directive.value() {
                                None => None,
                                Some(s) => Some(s.to_owned()),
                            }
                        }
                        "ExecStopFailureTimeout" => {
                            scenario_description.exec_stop_failure_timeout = match directive.value() {
                                None => None,
                                Some(s) => Some(Self::parse_time(s)?),
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

    /// Returns true if this scenario is supported on the named jig.
    pub fn supports_jig(&self, name: &UnitName) -> bool {
        self.jigs.contains(name)
    }

    /// Determine if a unit is compatible with this system.
    pub fn is_compatible(&self,
                         manager: &UnitManager,
                         _: &Config)
                         -> Result<(Vec<UnitName>, Dependy<UnitName>), UnitIncompatibleReason> {
        // If there is at least one jig present, ensure that it is loaded.
        if self.jigs.len() > 0 {
            let mut loaded = false;
            for jig_name in &self.jigs {
                if manager.jig_is_loaded(&jig_name) {
                    loaded = true;
                }
            }
            if !loaded {
                return Err(UnitIncompatibleReason::IncompatibleJig);
            }
        }

        // Build the dependency graph, but don't use the result.
        // This is because right now, we're just concerned with
        // whether the dependencies are satisfied.
        self.get_test_order(manager)
    }

    pub fn load(&self,
                  manager: &UnitManager,
                  config: &Config)
                  -> Result<Scenario, UnitIncompatibleReason> {
        let (test_order, graph) = self.is_compatible(manager, config)?;
        Ok(Scenario::new(self, test_order, manager, graph))
    }

    pub fn get_test_order(&self,
                          manager: &UnitManager)
                          -> Result<(Vec<UnitName>, Dependy<UnitName>), UnitIncompatibleReason> {

        // Create a new dependency graph
        let mut graph = Dependy::new();

        // Add each possible test into the dependency graph
        {
            let tests_rc = manager.get_tests();
            let tests = tests_rc.borrow();
            for (test_name, test) in tests.iter() {
                if self.assumptions.contains(test_name) {
                    let assumption_dep = AssumptionDependency::new(test_name.clone());
                    graph.add_dependency(&assumption_dep);
                } else {
                    graph.add_dependency(&*test.borrow());
                }
            }
        }

        let mut test_names = vec![];
        for test_name in &self.tests {
            test_names.push(test_name.clone());
        }

        let test_sequence = graph.resolve_named_dependencies(&test_names)?;
        let mut test_order = vec![];
        for test_name in test_sequence {
            // Only add the test to the test order if it's not an assumption.
            if !self.assumptions.contains(&test_name) {
                test_order.push(test_name);
            }
        }

        // let test_order = trimmed_order;
        Ok((test_order, graph))
    }
}

#[derive(Clone, PartialEq, Debug)]
enum ScenarioState {
    /// The scenario has been loaded, and is ready to run.
    Idle,

    /// The scenario has started, but is waiting for ExecStart to finish
    PreStart,

    /// The scenario is running, and is on step (u32)
    Running(usize),

    /// The scenario has succeeded, and is running the ExecStopSuccess step
    PostSuccess,

    /// The scenario has failed, and is running the ExecStopFailure step
    PostFailure,

    /// The test has succeeded or failed
    TestFinished,
}

pub struct Scenario {
    /// A reference to the scenario description that constructed this test.
    description: ScenarioDescription,

    /// A list of tests, in the order in which they will run.
    test_sequence: Vec<Rc<RefCell<Test>>>,

    /// A pointer to the tests that are part of this scenario.
    tests: HashMap<UnitName, Rc<RefCell<Test>>>,

    /// The results of each individual test.
    test_states: HashMap<UnitName, Rc<RefCell<TestState>>>,

    /// The result of the ExecStart run program (if any).
    exec_start_state: Rc<RefCell<TestState>>,
    
    /// How many tests have failed in this particular run.
    failures: Rc<RefCell<u32>>,

    /// The current state of the scenario, when activated.
    state: Rc<RefCell<ScenarioState>>,

    /// The current working directory, based on the description, jig, and config.
    working_directory: Rc<RefCell<Option<PathBuf>>>,

    /// The dependency graph of tests.
    graph: Dependy<UnitName>,

    /// When the test was started.
    start_time: Instant,

    /// The currently-executing program (if any)
    program: Rc<RefCell<Option<Running>>>,
}

impl Scenario {
    fn new(desc: &ScenarioDescription,
               test_order: Vec<UnitName>,
               manager: &UnitManager,
               graph: Dependy<UnitName>)
               -> Scenario {

        let mut tests = HashMap::new();
        let mut test_sequence = vec![];
        let mut test_state = HashMap::new();

        for test_name in test_order {
            let test = manager.get_test_named(&test_name).expect("Unable to check out requested test from library");
            test_sequence.push(test.clone());
            test_state.insert(test_name.clone(), Rc::new(RefCell::new(TestState::Pending)));
            tests.insert(test_name, test);
        }

        Scenario {
            description: desc.clone(),
            tests: tests,
            test_sequence: test_sequence,
            test_states: test_state,
            exec_start_state: Rc::new(RefCell::new(TestState::Pending)),
            state: Rc::new(RefCell::new(ScenarioState::Idle)),
            working_directory: Rc::new(RefCell::new(None)),
            failures: Rc::new(RefCell::new(0)),
            graph: graph,
            start_time: Instant::now(),
            program: Rc::new(RefCell::new(None)),
        }
    }

    pub fn test_sequence(&self) -> Vec<UnitName> {
        let mut test_sequence = vec![];
        for test in &self.test_sequence {
            test_sequence.push(test.borrow().id().clone());
        }
        test_sequence
    }

    pub fn tests(&self) -> &HashMap<UnitName, Rc<RefCell<Test>>> {
        &self.tests
    }

    pub fn id(&self) -> &UnitName {
        &self.description.id
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

        // Start afresh and reset our failure count.
        *self.failures.borrow_mut() = 0;
        self.start_time = Instant::now();
        *self.state.borrow_mut() = ScenarioState::Idle;
        *self.exec_start_state.borrow_mut() = TestState::Pending;
        for (_, item) in &self.test_states {
            *item.borrow_mut() = TestState::Pending;
        }

        // Re-assign our working directory.
        let mut wd = None;
        if let Some(ref d) = self.description.working_directory {
            wd = Some(d.clone());
        }
        if wd.is_none() && manager.get_current_jig().is_some() {
            let d = manager.get_current_jig().unwrap();
            let d = d.borrow();
            if d.working_directory().is_some() {
                wd = d.working_directory().clone();
            }
        }
        if wd.is_none() {
            wd = Some(config.working_directory().clone())
        };
        *self.working_directory.borrow_mut() = wd;

        // Cause the scenario to move to the next phase.
        ctrl.send(ManagerControlMessage::new(self.id(), ManagerControlMessageContents::AdvanceScenario(0))).ok();

        Ok(())
    }

    pub fn deactivate(&self) -> Result<(), UnitDeactivateError> {
        Ok(())
    }

    pub fn uses_test(&self, test_name: &UnitName) -> bool {
        self.tests.get(test_name).is_some()
    }

    pub fn name(&self) -> &String {
        &self.description.name
    }

    pub fn description(&self) -> &String {
        &self.description.description
    }

    // Given the current state, figure out the next test to run (if any)
    pub fn advance(&self, last_result: i32, ctrl: &Sender<ManagerControlMessage>) {
        let current_state = self.state.borrow().clone();

        // Run the test's stop() command if we just ran a test.
        match current_state {
            ScenarioState::Running(step) => {
                let result = match last_result {
                    0 => TestState::Pass,
                    r => TestState::Fail(format!("test exited with {}", r)),
                };
                *self.test_states.get(self.test_sequence[step].borrow().id()).unwrap().borrow_mut() = result;
                /* XXX Run the test's STOP command */
//                self.test_sequence[step]
//                    .stop(&*self.working_directory.lock().unwrap())
            }
            ScenarioState::PreStart => {
                match last_result {
                    0 => *self.exec_start_state.borrow_mut() = TestState::Pass,
                    r => *self.exec_start_state.borrow_mut() = TestState::Fail(format!("test exited with {}", r)),
                }
            }
            _ => (),
        }

        let new_state = self.find_next_state(current_state, ctrl);

        match new_state {
            // We generally shouldn't transition to the Idle state.
            ScenarioState::Idle => (),

            // If we want to run a preroll command and it fails, log it and start the tests.
            ScenarioState::PreStart => {
                // Unwrap because we've already validated it exists by setting the state to PreStart.
                let cmd = &self.description.exec_start.clone().unwrap();
                self.run_support_cmd(cmd,
                                     ctrl,
                                     &self.description.exec_start_timeout,
                                     "execstart");
            }
            ScenarioState::Running(next_step) => {
                let ref test = self.test_sequence[next_step].borrow();
                let test_timeout = test.timeout();
                let test_max_time = self.make_timeout(test_timeout);
                //test.start(&*self.working_directory.lock().unwrap(), test_max_time);
            }
            ScenarioState::PostSuccess => {
                let cmd = &self.description.exec_stop_success.clone().unwrap();
                self.run_support_cmd(cmd,
                                     ctrl,
                                     &self.description.exec_stop_success_timeout,
                                     "execstopsuccess");
            }
            ScenarioState::PostFailure => {
                let cmd = &self.description.exec_stop_failure.clone().unwrap();
                self.run_support_cmd(cmd,
                                     ctrl,
                                     &self.description.exec_stop_failure_timeout,
                                     "execstopfailure");
            }

            // If we're transitioning to the Finshed state, it means we just finished
            // running some tests.  Broadcast the result.
            ScenarioState::TestFinished => self.finish_scenario(ctrl),
        }
    }

    /// Run a support command (i.e. ExecStart, ExecStopSuccess, or ExecStopFailure).
    /// Will emit an AdvanceScenario message upon completion.
    fn run_support_cmd(&self, cmd: &String, ctrl: &Sender<ManagerControlMessage>, timeout: &Option<Duration>, _testname: &str) {
        let mut cmd = Runny::new(cmd);
        if let Some(timeout) = *timeout {
            cmd.timeout(timeout);
        }
        cmd.directory(&*self.working_directory.borrow());
        let mut running = match cmd.start() {
            Ok(o) => o,
            Err(e) => unimplemented!(),
        };

        self.log_output(ctrl, &mut running);

        // Keep a waiter around in a separate thread to send that AdvanceScenario message upon completion.
        let thr_waiter = running.waiter();
        let thr_control = ctrl.clone();
        let id = self.id().clone();
        thread::spawn(move || {
            thr_waiter.wait();
            thr_control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::AdvanceScenario(thr_waiter.result()))).ok();
        });

        *self.program.borrow_mut() = Some(running);
    }

    fn log_output(&self, control: &Sender<ManagerControlMessage>, process: &mut Running) {
        
        let stdout = process.take_output();
        let thr_control = control.clone();
        let id = self.id().clone();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let line = line.expect("Unable to get next line");
                if let Err(_) = thr_control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::Log(line))) {
                    break;
                }
            }
        });

        let stderr = process.take_error();
        let thr_control = control.clone();
        let id = self.id().clone();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let line = line.expect("Unable to get next line");
                if let Err(_) = thr_control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::LogError(line))) {
                    break;
                }
            }
        });
    }

    /// Find the next state.
    /// If we're idle, start the test.
    /// The state order goes:
    /// Idle -> [PreStart] -> Test(0) -> ... -> Test(n) -> [PostSuccess/Fail] -> Idle
    ///
    fn find_next_state(&self, current_state: ScenarioState, ctrl: &Sender<ManagerControlMessage>) -> ScenarioState {

        let test_count = self.tests.len();
        let failure_count = *self.failures.borrow();

        let new_state = match current_state {
            ScenarioState::Idle => {
                // Reset the number of errors.
                *self.failures.borrow_mut() = 0;
                for test in &self.test_sequence {
                    test.borrow().pending();
                }

                //self.broadcast(BroadcastMessageContents::Start(self.id().to_string()));
                ScenarioState::PreStart
            }

            // If we've just run the PreStart command, see if we need
            // to run test 0, or skip straight to Success.
            ScenarioState::PreStart => ScenarioState::Running(0),

            // If we just finished running a test, determine the next test to run.
            ScenarioState::Running(i) if (i + 1) < test_count => ScenarioState::Running(i + 1),
            ScenarioState::Running(i) if (i + 1) >= test_count && failure_count > 0 => {
                ScenarioState::PostFailure
            }
            ScenarioState::Running(i) if (i + 1) >= test_count && failure_count == 0 => {
                ScenarioState::PostSuccess
            }
            ScenarioState::Running(i) => {
                panic!("Got into a weird state. Running({}), test_count: {}, failure_count: {}",
                       i,
                       test_count,
                       failure_count)
            }
            ScenarioState::PostFailure => ScenarioState::TestFinished,
            ScenarioState::PostSuccess => ScenarioState::TestFinished,
            ScenarioState::TestFinished => ScenarioState::TestFinished,
        };

        // If it's an acceptable new state, set that.  Otherwise, recurse
        // and try the next state.
        if self.is_state_okay(&new_state, ctrl) {
            *self.state.borrow_mut() = new_state.clone();
            new_state
        } else {
            self.find_next_state(new_state, ctrl)
        }
    }

    /// Check the proposed state to make sure it's acceptable.
    /// Reasons it might not be acceptable might be because there
    /// is no exec_start and the new state is PreStart, or because
    /// the new state is on a test whose requirements are not met.
    fn is_state_okay(&self, new_state: &ScenarioState, ctrl: &Sender<ManagerControlMessage>) -> bool {

        match *new_state {
            // We can always enter the idle state.
            ScenarioState::Idle => true,

            // Run an exec_start command before we run the first test.
            ScenarioState::PreStart => self.description.exec_start.is_some(),

            // Run a given test.
            ScenarioState::Running(i) => {
                let tests = &self.test_sequence;
                let test = tests[i].borrow();
                let test_name = test.id();
                if self.scenario_timed_out() {
                    false
                } else if i >= self.tests.len() {
                    false
                } else if let TestState::Fail(ref _x) = *self.exec_start_state.borrow() {
                    // If the preroll command failed, then abort.
                    false
                } else if tests[i].borrow().state() != TestState::Pending {
                    // If the test isn't Pending (i.e. if it's skipped or failed), don't run it.
                    false
                }
                // Make sure all required dependencies succeeded.
                else if !self.all_dependencies_succeeded(&test_name) {
                    tests[i].borrow().skip();
                    ctrl.send(ManagerControlMessage::new(self.id(), ManagerControlMessageContents::Skip(tests[i].borrow().id().clone(), "dependency failed".to_owned()))).ok();
                    false
                } else {
                    true
                }
            }

            // Run a script on scenario success.
            ScenarioState::PostSuccess => self.description.exec_stop_success.is_some(),

            // Run a script on scenario failure.
            ScenarioState::PostFailure => self.description.exec_stop_failure.is_some(),

            // Presumably we can always finish a test.
            ScenarioState::TestFinished => true,
        }
    }

    fn all_dependencies_succeeded(&self, test_name: &UnitName) -> bool {
        for parent_name in self.graph.required_parents_of_named(test_name) {
            if self.description.assumptions.contains(parent_name) {
                return true;
            }

            let result = self.tests[parent_name].borrow().state();

            // If the dependent test did not succeed, then at least
            // one dependency failed.
            // The test may also be Running, in case it's a Daemon.
            if result != TestState::Pass && result != TestState::Running {
                return false;
            }

            if !self.all_dependencies_succeeded(parent_name) {
                return false;
            }
        }
        true
    }

    fn scenario_timed_out(&self) -> bool {
        match self.description.timeout {
            None => false,
            Some(timeout) => {
                let now = Instant::now();
                let scenario_elapsed_time = now.duration_since(self.start_time);
                scenario_elapsed_time >= timeout
            }
        }
    }

    fn make_timeout(&self, test_max_time: &Option<Duration>) -> Option<Duration> {
        let now = Instant::now();
        let scenario_elapsed_time = now.duration_since(self.start_time);

        // If the test would take longer than the scenario has left, limit the test time.
        if let Some(test_max_time) = *test_max_time {
            if let Some(timeout) = self.description.timeout {
                if (test_max_time + scenario_elapsed_time) > timeout {
                    Some(timeout - scenario_elapsed_time)
                } else {
                    Some(test_max_time)
                }
            } else {
                Some(test_max_time)
            }
        } else {
            None
        }
    }

    // Post messages and terminate tests.
    pub fn finish_scenario(&self, ctrl: &Sender<ManagerControlMessage>) {
        let failures = *self.failures.borrow();
        for test in &self.test_sequence {
            test.borrow().terminate();
        }
        if failures > 0 {
            ctrl.send(ManagerControlMessage::new(self.id(),
                                                ManagerControlMessageContents::Finished(failures + 500, "at least one test failed".to_owned()))).ok();
        } else {
            ctrl.send(ManagerControlMessage::new(self.id(),
                                                ManagerControlMessageContents::Finished(200, "all tests passed".to_owned()))).ok();
        }
    }
}