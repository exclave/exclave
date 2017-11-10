// The UnitManager contains all units that are Selected.  This includes
// units that are Active.
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use config::Config;
use unit::{UnitName, UnitKind, UnitActivateError, UnitDeactivateError, UnitSelectError, UnitDeselectError, UnitIncompatibleReason};
use unitbroadcaster::{UnitBroadcaster, UnitEvent, UnitStatusEvent, UnitStatus, LogEntry};
use units::interface::{Interface, InterfaceDescription};
use units::jig::{Jig, JigDescription};
use units::scenario::{Scenario, ScenarioDescription};
use units::test::{Test, TestDescription};

macro_rules! load {
    ($slf:ident, $dest:ident, $desc:ident) => {
        {
            // If the item exists in the array already, then it is active and will be deselected first.
            if $slf.$dest.borrow().contains_key($desc.id()) {
                // Deselect the old one it before unloading
                $slf.deselect($desc.id(), "reloading");
            };
            // "Load" the Unit, which means we can select or activate it later on.
            match $desc.load($slf, &*$slf.cfg.lock().unwrap()) {
                Ok(o) => {
                    $slf.$dest.borrow_mut().insert($desc.id().clone(), Rc::new(RefCell::new(o)));

                    // Announce the fact that the unit was loaded successfully.
                    $slf.bc
                        .broadcast(&UnitEvent::Status(UnitStatusEvent::new_loaded($desc.id())));

                    Ok($desc.id().clone())
                }
                Err(e) => {
                    $slf.bc.broadcast(
                        &UnitEvent::Status(UnitStatusEvent::new_unit_incompatible(
                            $desc.id(),
                            format!("{}", e),
                        )),
                    );
                    Err(e)
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum FieldType {
    Name,
    Description,
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &FieldType::Name => write!(f, "name"),
            &FieldType::Description => write!(f, "description"),
        }
    }
}

/// Messages for Library -> Unit communication
#[derive(Debug, Clone)]
pub enum ManagerStatusMessage {
    /// Return the first name of the jig we're running on.
    Jig(Option<UnitName> /* Name of the jig (if one is selected) */),

    /// Return a list of known scenarios.
    Scenarios(Vec<UnitName>),

    /// Return the currently-selected scenario, if any
    Scenario(Option<UnitName>),

    /// Return a list of tests in a scenario.
    Tests(UnitName /* Scenario name */, Vec<UnitName> /* List of tests */),

    /// Greeting identifying the server.
    Hello(String /* Server identification name */),

    /// Describes a Type of a particular Field on a given Unit
    Describe(UnitName, FieldType, String /* Value */),

    /// A log message from one of the units, or the system itself.
    Log(LogEntry),

    /// A test has started running,
    Running(UnitName),

    /// Indicates that a test passed successfully.
    Pass(UnitName, String /* log message */),

    /// Indicates that a test failed for some reason.
    Fail(UnitName, i32 /* return code */, String /* log message */),

    /// Indicates that a test was skipped for some reason.
    Skipped(UnitName, String /* reason */),

    /// Sent when a scenario has finished running.
    Finished(UnitName /* Scenario name */, u32 /* Result code */, String /* Reason for finishing */),

}

/// Messages for Unit -> Library communication
#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub enum ManagerControlMessageContents {
    /// Get the current Jig
    Jig,

    /// Get a list of compatible, Selected scenarios.
    Scenarios,

    /// Select a specific scenario.
    Scenario(UnitName /* Scenario name */),

    /// Get a list of tests, either from the current scenario (None) or a specific scenario (Some)
    Tests(Option<UnitName>),

    /// An error message from a particular interface.
    Error(String /* Error message contents */),

    /// Sent to a unit when it is first loaded, including "HELLO" messages.
    InitialGreeting,

    /// Tells the Manager to advance the current scenario.
    AdvanceScenario(i32 /* result code of last step */),

    /// Indicates the child (Interface, Test, etc.) has exited.
    ChildExited,

    /// Client sent an unimplemented message.
    Unimplemented(String /* verb */, String /* rest of line */),

    /// Send an INFO message to the logging system
    Log(String /* log message */),

    /// Send an ERROR message to the logging system
    LogError(String /* log message */),

    /// Start running a scenario, or the default scenario if None
    StartScenario(Option<UnitName>),

    /// Start running a given test.
    StartTest(UnitName),

    /// Stop running a given test.
    StopTest(UnitName),

    /// Sent when a test has started running.
    TestStarted,

    /// Indicates that a test was skipped, and why.
    Skip(UnitName, String /* reason */),

    /// Indicates that a scenario has finished, and how many tests passed.
    ScenarioFinished(u32 /* Finish code */, String /* Informative message */),

    /// Indicates that a test has finished
    TestFinished(i32 /* Finish code */, String /* The last printed line */),
}

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct ManagerControlMessage {
    sender: UnitName,
    contents: ManagerControlMessageContents,
}

impl ManagerControlMessage {
    pub fn new(id: &UnitName, contents: ManagerControlMessageContents) -> Self {
        ManagerControlMessage {
            sender: id.clone(),
            contents: contents,
        }
    }
}

pub struct UnitManager {
    cfg: Arc<Mutex<Config>>,
    bc: UnitBroadcaster,

    /// Loaded Interfaces, available for selection and activation.
    interfaces: RefCell<HashMap<UnitName, Rc<RefCell<Interface>>>>,

    /// Loaded Jigs, available for selection and activation.
    jigs: RefCell<HashMap<UnitName, Rc<RefCell<Jig>>>>,

    /// Loaded Scenarios, available for selected and activation.
    scenarios: Rc<RefCell<HashMap<UnitName, Rc<RefCell<Scenario>>>>>,

    /// Loaded Tests, available for selection and activation.
    tests: Rc<RefCell<HashMap<UnitName, Rc<RefCell<Test>>>>>,

    /// Prototypical message sender that will be cloned and passed to each new unit.
    control_sender: Sender<ManagerControlMessage>,

    /// The currently-selected Scenario, if any
    current_scenario: Rc<RefCell<Option<Rc<RefCell<Scenario>>>>>,

    /// The currently-selected Jig, if any
    current_jig: Rc<RefCell<Option<Rc<RefCell<Jig>>>>>,

    /// A list of selected units.
    selected: Rc<RefCell<HashMap<UnitName, ()>>>,

    /// A list of active units.  These units must also be selected.
    active: Rc<RefCell<HashMap<UnitName, ()>>>,
}

impl UnitManager {
    pub fn new(broadcaster: &UnitBroadcaster, config: &Arc<Mutex<Config>>) -> Self {
        let (sender, receiver) = channel();

        let monitor_broadcaster = broadcaster.clone();
        thread::spawn(move || Self::control_message_monitor(receiver, monitor_broadcaster));

        UnitManager {
            cfg: config.clone(),
            bc: broadcaster.clone(),

            interfaces: RefCell::new(HashMap::new()),
            jigs: RefCell::new(HashMap::new()),
            scenarios: Rc::new(RefCell::new(HashMap::new())),
            tests: Rc::new(RefCell::new(HashMap::new())),

            selected: Rc::new(RefCell::new(HashMap::new())),
            active: Rc::new(RefCell::new(HashMap::new())),

            current_scenario: Rc::new(RefCell::new(None)),
            current_jig: Rc::new(RefCell::new(None)),

            control_sender: sender,
        }
    }

    /// Runs in a separate thread and consolidates control messages
    fn control_message_monitor(receiver: Receiver<ManagerControlMessage>, broadcaster: UnitBroadcaster) {
        while let Ok(msg) = receiver.recv() {
            broadcaster.broadcast(&UnitEvent::ManagerRequest(msg));
        }
    }

    pub fn get_control_channel(&self) -> Sender<ManagerControlMessage> {
        self.control_sender.clone()
    }

    pub fn load_interface(&self, description: &InterfaceDescription) -> Result<UnitName, UnitIncompatibleReason> {
        load!(self, interfaces, description)
    }

    pub fn load_test(&self, desceription: &TestDescription) -> Result<UnitName, UnitIncompatibleReason> {
        load!(self, tests, desceription)
    }

    pub fn load_jig(&self, desceription: &JigDescription) -> Result<UnitName, UnitIncompatibleReason> {
        load!(self, jigs, desceription)
    }

    pub fn load_scenario(&self, desceription: &ScenarioDescription) -> Result<UnitName, UnitIncompatibleReason> {
        load!(self, scenarios, desceription)
    }

    pub fn select(&self, id: &UnitName) {
        // Don't select already-selected units.
        if self.selected.borrow().contains_key(id) {
            return;
        }

        let result = match *id.kind() {
            UnitKind::Interface => self.select_interface(id),
            UnitKind::Jig => self.select_jig(id),
            UnitKind::Scenario => self.select_scenario(id),
            UnitKind::Test => self.select_test(id),
            UnitKind::Internal => Ok(()),
        };

        // Announce that the interface was successfully started.
        match result {
            Ok(_) => {
                self.selected.borrow_mut().insert(id.clone(), ());
                self.bc.broadcast(&UnitEvent::Status(UnitStatusEvent::new_selected(id)));
            },
            Err(e) =>
               self.bc.broadcast(
                    &UnitEvent::Status(UnitStatusEvent::new_select_failed(id, format!("{}", e)))),
        }
    }

    pub fn select_scenario(&self, id: &UnitName) -> Result<(), UnitSelectError> {
        let new_scenario = match self.scenarios.borrow().get(id) {
            Some(s) => s.clone(),
            None => return Err(UnitSelectError::UnitNotFound),
        };

        // If there is an existing current scenario, check to see if the ID matches.
        // If so, there is nothing to do.
        // If not, deselect it.
        // There Can Only Be One.
        let deselct_id_opt = if let Some(ref old_scenario) = *self.current_scenario.borrow() {
            if old_scenario.borrow().id() == id {
                // Units match, so do nothing.
                return Ok(());
            }
            Some(old_scenario.borrow().id().clone())
        } else {
            None
        };

        if let Some(ref old_id) = deselct_id_opt {
            self.deselect(old_id, "switching to a new scenario");
        }
        
        // Select this scenario.
        new_scenario.borrow_mut().select()?;
        *self.current_scenario.borrow_mut() = Some(new_scenario.clone());

        // Now select every test associated with the scenario.
        for test_id in new_scenario.borrow().test_sequence() {
            //self.tests.borrow().get(test_id).unwrap().borrow().select()
        }
        Ok(())
    }

    fn select_jig(&self, id: &UnitName) -> Result<(), UnitSelectError> {
        let new_jig = match self.jigs.borrow().get(id) {
            Some(s) => s.clone(),
            None => return Err(UnitSelectError::UnitNotFound),
        };

        // If there is an existing current jig, check to see if the ID matches.
        // If so, there is nothing to do.
        // If not, deselect it.
        // There Can Only Be One.
        let deselct_id_opt = if let Some(ref old_jig) = *self.current_jig.borrow() {
            if old_jig.borrow().id() == id {
                // Units match, so do nothing.
                return Ok(());
            }
            Some(old_jig.borrow().id().clone())
        } else {
            None
        };
        if let Some(ref old_id) = deselct_id_opt {
            self.deselect(old_id, "switching to a new jig");
        }

        // Select this jig.
        new_jig.borrow_mut().select()?;
        *self.current_jig.borrow_mut() = Some(new_jig.clone());

        // If this jig has a default scenario, select that too.
        if let Some(ref scenario_name) = *new_jig.borrow().default_scenario() {
            self.select(scenario_name);
        }

        Ok(())
    }

    fn select_test(&self, id: &UnitName) -> Result<(), UnitSelectError> { 
        match self.tests.borrow().get(id) {
            Some(ref s) => s.borrow_mut().select(),
            None => Err(UnitSelectError::UnitNotFound),
        }
    }

    fn select_interface(&self, id: &UnitName) -> Result<(), UnitSelectError> {
        match self.interfaces.borrow().get(id) {
            Some(ref s) => s.borrow_mut().select(),
            None => Err(UnitSelectError::UnitNotFound),
        }
    }

    pub fn deselect(&self, id: &UnitName, reason: &str) {
        self.deactivate(id, "unit is being deselcted");

        // Don't deselect a unit that hasn't been selected.
        if ! self.selected.borrow().contains_key(id) {
            return;
        }

        // Remove the item from its associated Rc array.
        // Note that because these are Rcs, they may live on for a little while
        // longer as references in other objects.
        let result = match id.kind() {
            &UnitKind::Interface => self.deselect_interface(id),
            &UnitKind::Test => self.deselect_test(id),
            &UnitKind::Scenario => self.deselect_scenario(id),
            &UnitKind::Jig => self.deselect_jig(id),
            &UnitKind::Internal => Ok(()),
        };

        // A not-okay result is fine, it just means we couldn't find the unit.
        if result.is_ok() {
            self.selected.borrow_mut().remove(id);
            self.bc.broadcast(&UnitEvent::Status(UnitStatusEvent::new_deselected(id, reason.to_owned())));
        }
    }

    fn deselect_test(&self, id: &UnitName) -> Result<(), UnitDeselectError> {
        match self.tests.borrow().get(id) {
            Some(ref s) => s.borrow_mut().deselect(),
            None => Err(UnitDeselectError::UnitNotFound),
        }
    }

    fn deselect_interface(&self, id: &UnitName) -> Result<(), UnitDeselectError> {
        match self.interfaces.borrow().get(id) {
            Some(ref s) => s.borrow_mut().deselect(),
            None => Err(UnitDeselectError::UnitNotFound),
        }
    }

    fn deselect_jig(&self, id: &UnitName) -> Result<(), UnitDeselectError> {
        // If the specified jig isn't the current jig, then there's nothing to do.
        let mut current_jig_opt = self.current_jig.borrow_mut();

        let current_jig = match *current_jig_opt {
            None => return Ok(()),
            Some(ref s) => {
                let current_jig = s.borrow();
                if current_jig.id() != id {
                    return Ok(());
                }
                s.clone()
            }
        };

        // If there is a default scenario, make sure it's deselected.
        if let Some(new_scenario_id) = current_jig.borrow().default_scenario().clone() {
            self.deselect(&new_scenario_id, "jig is deselecting");
        }

        current_jig.borrow_mut().deselect()?;
        *current_jig_opt = None;
        Ok(())
    }

    fn deselect_scenario(&self, id: &UnitName) -> Result<(), UnitDeselectError> {
        // If the specified scenario isn't the current scenario, then there's nothing to do.
        match *self.current_scenario.borrow() {
            None => return Ok(()),
            Some(ref s) => {
                let current_scenario = s.borrow();
                if current_scenario.id() != id {
                    return Ok(());
                }
            }
        }
        if let Some(ref old_scenario) = self.current_scenario.borrow_mut().take() {
            old_scenario.borrow_mut().deselect()?;
        }
        Ok(())
    }

    pub fn activate(&self, id: &UnitName) {
        self.select(id);

        // Don't activate a unit that is already active.
        if self.active.borrow().contains_key(id) {
            return;
        }

        // If the unit still hasn't been selected, don't activate it.
        if ! self.selected.borrow().contains_key(id) {
            return;
        }

        let result = match *id.kind() {
            UnitKind::Interface => self.activate_interface(id),
            UnitKind::Jig => self.activate_jig(id),
            UnitKind::Scenario => self.activate_scenario(id),
            UnitKind::Test => self.activate_test(id),
            UnitKind::Internal => Ok(()),
        };

        // Announce that the interface was successfully started.
        match result {
            Ok(_) => {
                self.active.borrow_mut().insert(id.clone(), ());
                self.bc.broadcast(&UnitEvent::Status(UnitStatusEvent::new_active(id)))
            },
            Err(e) =>
               self.bc.broadcast(
                    &UnitEvent::Status(UnitStatusEvent::new_active_failed(id, format!("unable to activate: {}", e)))),
        }
    }

    /// If there are unselected defaults, activate them.
    /// For example, if there is no current Jig, activate the first Jig we find.
    /// Likewise, if there is no selected Scenario, select the first scenario we find.
    pub fn refresh_defaults(&self) {
        // Activate a "random" available jig.
        if self.current_jig.borrow().is_none() && !self.jigs.borrow().is_empty() {
            let new_jig_id = self.jigs.borrow().keys().next().unwrap().clone();
            self.activate(&new_jig_id);
        }

        // If there is no current scenario, select a random one.
        if self.current_scenario.borrow().is_none() && !self.scenarios.borrow().is_empty() {
            let new_scenario_id = self.scenarios.borrow().keys().next().unwrap().clone();
            self.select(&new_scenario_id);
        }
    }

    fn activate_interface(&self, id: &UnitName) -> Result<(), UnitActivateError> {
        // Activate the interface, which actually starts it up.
        match self.interfaces.borrow().get(id) {
            Some(i) => i.borrow_mut().activate(self, &*self.cfg.lock().unwrap()),
            None => return Err(UnitActivateError::UnitNotFound),
        }
    }

    /// Set the new jig as "Active".
    /// The jig must already be set as the current jig.
    fn activate_jig(&self, id: &UnitName) -> Result<(), UnitActivateError> {
        let current_jig_opt = self.current_jig.borrow();

        match *current_jig_opt {
            None => Err(UnitActivateError::UnitNotSelected),
            Some(ref s) => if s.borrow_mut().id() != id {
                Err(UnitActivateError::UnitNotSelected)
            } else {
                // Activate this jig.
                s.borrow_mut().activate(self, &*self.cfg.lock().unwrap())
            }
        }
    }

    /// Set the specified scenario as "Active".
    /// This actually runs the scenario.
    fn activate_scenario(&self, id: &UnitName) -> Result<(), UnitActivateError> {
        let current_opt = self.current_scenario.borrow();

        match *current_opt {
            None => Err(UnitActivateError::UnitNotSelected),
            Some(ref s) => if s.borrow().id() != id {
                Err(UnitActivateError::UnitNotSelected)
            } else {
                // Activate this scenario.
                s.borrow_mut().activate(self, &*self.cfg.lock().unwrap())
            }
        }
    }

    fn activate_test(&self, id: &UnitName) -> Result<(), UnitActivateError> {
        match self.tests.borrow().get(id) {
            None => Err(UnitActivateError::UnitNotFound),
            Some(ref s) => s.borrow_mut().activate(self, &*self.cfg.lock().unwrap()),
        }
    }

    pub fn deactivate(&self, id: &UnitName, reason: &str) {

        // Don't deactivate an inactive unit.
        if ! self.active.borrow().contains_key(id) {
            return;
        }

        let result = match *id.kind() {
            UnitKind::Interface => self.deactivate_interface(id),
            UnitKind::Jig => self.deactivate_jig(id),
            UnitKind::Scenario => self.deactivate_scenario(id),
            UnitKind::Test => self.deactivate_test(id),
            UnitKind::Internal => Ok(()),
        };
        match result {
            Ok(_) => {
                self.active.borrow_mut().remove(id);
                self.bc.broadcast(&UnitEvent::Status(UnitStatusEvent::new_deactivate_success(id, reason.to_owned())))
            },
            Err(e) =>
                self.bc.broadcast(
                        &UnitEvent::Status(UnitStatusEvent::new_deactivate_failure(id, format!("unable to deactivate: {}", e)))),
        }
    }

    fn deactivate_interface(&self, id: &UnitName) -> Result<(), UnitDeactivateError> {
        let interfaces = self.interfaces.borrow();
        match interfaces.get(id) {
            None => return Err(UnitDeactivateError::UnitNotFound),
            Some(interface) => interface.borrow_mut().deactivate(),
        }
    }

    fn deactivate_test(&self, id: &UnitName) -> Result<(), UnitDeactivateError> {
        let tests = self.tests.borrow();
        match tests.get(id) {
            None => return Err(UnitDeactivateError::UnitNotFound),
            Some(test) => test.borrow_mut().deactivate(),
        }
    }

    fn deactivate_scenario(&self, id: &UnitName) -> Result<(), UnitDeactivateError> {
        let current_scenario_opt = self.current_scenario.borrow_mut();

        // If the specified scenario isn't the current scenario, then there's nothing to do.
        match *current_scenario_opt {
            None => Ok(()),
            Some(ref s) => {
                let current_scenario = s.borrow_mut();
                if current_scenario.id() != id {
                    Ok(())
                }
                else {
                    current_scenario.deactivate()
                }
            }
        }
    }

    fn deactivate_jig(&self, id: &UnitName) -> Result<(), UnitDeactivateError> {
        let current_opt = self.current_jig.borrow_mut();

        // If the specified jig isn't the current jig, then there's nothing to do.
        match *current_opt {
            None => Ok(()),
            Some(ref s) => {
                let current = s.borrow_mut();
                if current.id() != id {
                    Ok(())
                }
                else {
                    current.deactivate()
                }
            }
        }
    }

    pub fn unload(&self, id: &UnitName) {
        self.deselect(id, "unloading");
        match *id.kind() {
            UnitKind::Interface => self.unload_interface(id),
            UnitKind::Jig => self.unload_jig(id),
            UnitKind::Scenario => self.unload_scenario(id),
            UnitKind::Test => self.unload_test(id),
            UnitKind::Internal => (),
        }
    }
    
    fn unload_interface(&self, id: &UnitName) {
        self.deactivate(id, "interface is being unloaded");
        self.deselect(id, "interface is being unloaded");

        self.interfaces.borrow_mut().remove(id);
    }

    fn unload_jig(&self, id: &UnitName) {
        self.deactivate(id, "jig is being unloaded");
        self.deselect(id, "jig is being unloaded");

        self.jigs.borrow_mut().remove(id);
    }

    fn unload_test(&self, id: &UnitName) {
        self.deactivate(id, "test is being unloaded");
        self.deselect(id, "test is being unloaded");

        self.tests.borrow_mut().remove(id);
    }

    fn unload_scenario(&self, id: &UnitName) {
        self.deactivate(id, "scenario is being unloaded");
        self.deselect(id, "scenario is being unloaded");

        self.scenarios.borrow_mut().remove(id);
        self.broadcast_scenario_list();
    }

    pub fn get_scenario_named(&self, id: &UnitName) -> Option<Rc<RefCell<Scenario>>> {
        match self.scenarios.borrow().get(id) {
            None => None,
            Some(scenario) => Some(scenario.clone())
        }
    }

    pub fn get_test_named(&self, id: &UnitName) -> Option<Rc<RefCell<Test>>> {
        match self.tests.borrow().get(id) {
            None => None,
            Some(test) => Some(test.clone()),
        }
    }

    pub fn get_tests(&self) -> Rc<RefCell<HashMap<UnitName, Rc<RefCell<Test>>>>> {
        self.tests.clone()
    }

    pub fn get_scenarios(&self) -> Rc<RefCell<HashMap<UnitName, Rc<RefCell<Scenario>>>>> {
        self.scenarios.clone()
    }

     pub fn jig_is_loaded(&self, id: &UnitName) -> bool {
        self.jigs.borrow().get(id).is_some()
    }

    pub fn process_message(&self, msg: &UnitEvent) {
        match msg {
            &UnitEvent::ManagerRequest(ref req) => self.manager_request(req),
            &UnitEvent::Status(ref stat) => self.status_message(stat),
            &UnitEvent::Log(ref log) => {
                for (_, interface) in self.interfaces.borrow().iter() {
                    let log_status_msg = ManagerStatusMessage::Log(log.clone());
                    interface.borrow().output_message(log_status_msg).expect("Unable to pass message to client");
                }
            },
            _ => (),
        }
    }

    fn status_message(&self, msg: &UnitStatusEvent) {
        let &UnitStatusEvent {ref name, ref status} = msg;
        match status {
            &UnitStatus::Loaded => match name.kind() {
                &UnitKind::Jig => self.broadcast_jig_named(name),
                &UnitKind::Scenario => self.broadcast_scenario_named(name),
                &UnitKind::Test => self.broadcast_test_named(name),
                _ => (),
            },
            &UnitStatus::Selected => match name.kind() {
                &UnitKind::Jig => self.broadcast_selected_jig(),
                &UnitKind::Scenario => self.broadcast_selected_scenario(),
                _ => (),
            },
            _ => (),
        }
    }

    fn manager_request(&self, msg: &ManagerControlMessage) {
        let &ManagerControlMessage {sender: ref sender_name, contents: ref msg} = msg;

        match *msg {
            ManagerControlMessageContents::Scenarios => self.send_scenarios_to(sender_name),
            ManagerControlMessageContents::Tests(ref scenario_name) => self.send_tests_to(sender_name, scenario_name),
            ManagerControlMessageContents::Log(ref txt) => self.bc.broadcast(&UnitEvent::Log(LogEntry::new_info(sender_name.clone(), txt.clone()))),
            ManagerControlMessageContents::LogError(ref txt) => self.bc.broadcast(&UnitEvent::Log(LogEntry::new_error(sender_name.clone(), txt.clone()))),
            ManagerControlMessageContents::Scenario(ref new_scenario_name) => {
                if self.get_scenario_named(new_scenario_name).is_some() {
                    self.select(new_scenario_name);
                    self.broadcast_selected_scenario();
                } else {
                    self.bc.broadcast(&UnitEvent::Log(LogEntry::new_error(sender_name.clone(), format!("unable to find scenario {}", new_scenario_name))));
                }
            },
            ManagerControlMessageContents::Error(ref err) => {
                self.bc.broadcast(&UnitEvent::Log(LogEntry::new_error(sender_name.clone(), err.clone())));
            },
            ManagerControlMessageContents::Jig => self.send_jig_to(sender_name),
            ManagerControlMessageContents::InitialGreeting => {
                // Send some initial information to the client.
                self.send_hello_to(sender_name);
                self.send_jig_to(sender_name);
                self.send_scenarios_to(sender_name);
                // If there is a scenario selected, send that too.
                if let Some(ref sc) = *self.current_scenario.borrow() {
                    self.send_scenario_to(sender_name, &sc.borrow().id().clone());
                }
            },
            ManagerControlMessageContents::ChildExited => {
                self.bc.broadcast(&UnitEvent::Status(UnitStatusEvent::new_active_failed(sender_name, "Unit unexpectedly exited".to_owned())));
            },
            ManagerControlMessageContents::AdvanceScenario(result) => {
                match *self.current_scenario.borrow() {
                    None => (),
                    Some(ref current_scenario) => current_scenario.borrow_mut().advance(result, &self.control_sender),
                }
            },
            ManagerControlMessageContents::Unimplemented(ref verb, ref remainder) => {
                self.bc.broadcast(&UnitEvent::Log(LogEntry::new_error(sender_name.clone(), format!("unimplemented verb: {} (args: {})", verb, remainder))));
            },
            ManagerControlMessageContents::StartScenario(ref scenario_name_opt) => {
                let scenario_name = if let Some(ref scenario_name) = *scenario_name_opt {
                    self.select(scenario_name);
                    scenario_name.clone()
                } else {
                    match *self.current_scenario.borrow() {
                        None => {
                            self.bc.broadcast(&UnitEvent::Log(LogEntry::new_error(sender_name.clone(), "unable to start scenario: no scenario selected and no scenario specified".to_owned())));
                            return;
                        },
                        Some(ref scenario) => scenario.borrow().id().clone()
                    }
                };

                self.activate(&scenario_name);
            },
            ManagerControlMessageContents::Skip(ref test_name, ref reason) => {
                self.broadcast_skipped(test_name, reason);
            },
            ManagerControlMessageContents::TestStarted => {
                self.broadcast_message(ManagerStatusMessage::Running(sender_name.clone()));
            }
            ManagerControlMessageContents::TestFinished(result, ref message) => {
                self.broadcast_message(match result {
                    0 => ManagerStatusMessage::Pass(sender_name.clone(), message.clone()),
                    i => ManagerStatusMessage::Fail(sender_name.clone(), i, message.clone()),
                });
            }
            ManagerControlMessageContents::ScenarioFinished(code, ref message) => {
                self.broadcast_finished(sender_name, code, message);
            }
            ManagerControlMessageContents::StartTest(ref test_name) => {
                self.activate(test_name);
            }
            ManagerControlMessageContents::StopTest(ref test_name) => {
                self.deactivate(test_name, "controller requested test stop");
            }
        }
    }

    pub fn send_hello_to(&self, sender_name: &UnitName) {
        self.send_messages_to(sender_name, vec![ManagerStatusMessage::Hello("Jig/20 1.0".to_owned())]);
    }

    pub fn send_jig_to(&self, sender_name: &UnitName) {
        let messages = match *self.current_jig.borrow() {
            None => vec![ManagerStatusMessage::Jig(None)],
            Some(ref jig_rc) => {
                let jig = jig_rc.borrow();
                vec![
                    ManagerStatusMessage::Jig(Some(jig.id().clone())),
                    ManagerStatusMessage::Describe(jig.id().clone(), FieldType::Name, jig.name().clone()),
                    ManagerStatusMessage::Describe(jig.id().clone(), FieldType::Description, jig.description().clone())
                ]
            }
        };
        self.send_messages_to(sender_name, messages);
    }

    /// Send all available scenarios to the specified endpoint.
    pub fn send_scenarios_to(&self, sender_name: &UnitName) {
        let mut messages = vec![ManagerStatusMessage::Scenarios(self.scenarios.borrow().keys().map(|x| x.clone()).collect())];
        for (scenario_id, scenario) in self.scenarios.borrow().iter() {
            messages.push(ManagerStatusMessage::Describe(scenario_id.clone(), FieldType::Name, scenario.borrow().name().clone()));
            messages.push(ManagerStatusMessage::Describe(scenario_id.clone(), FieldType::Description, scenario.borrow().description().clone()));
        }
        self.send_messages_to(sender_name, messages);
    }

    pub fn send_scenario_to(&self, sender_name: &UnitName, scenario_name: &UnitName) {
        let messages = match self.scenarios.borrow().get(scenario_name) {
            None => vec![ManagerStatusMessage::Scenario(None)],
            Some(scenario_rc) => {
                let scenario = scenario_rc.borrow();
                let mut messages = vec![ManagerStatusMessage::Scenario(Some(scenario_name.clone()))];
                for (test_id, test_rc) in scenario.tests() {
                    let test = test_rc.borrow();
                    messages.push(ManagerStatusMessage::Describe(test_id.clone(), FieldType::Name, test.name().clone()));
                    messages.push(ManagerStatusMessage::Describe(test_id.clone(), FieldType::Description, test.description().clone()));
                }
                messages.push(ManagerStatusMessage::Tests(scenario.id().clone(), scenario.test_sequence()));
                messages
            }
        };
        self.send_messages_to(sender_name, messages);
    }

    /// Send a list of tests to the specified recipient.
    /// If no scenario name is specified, send the current scenario.
    pub fn send_tests_to(&self, sender_name: &UnitName, scenario_name_opt: &Option<UnitName>) {
        let scenario_id = match *scenario_name_opt {
            Some(ref n) => n.clone(),
            None => match *self.current_scenario.borrow() {
                Some(ref cs) => cs.borrow().id().clone(),
                None => {
                    self.bc.broadcast(&UnitEvent::Log(LogEntry::new_error(sender_name.clone(), "unable to list tests, no scenario specified and no scenario selected".to_owned())));
                    return;
                }
            }
        };
        let scenarios = self.scenarios.borrow();
        let scenario_rc_opt = scenarios.get(&scenario_id);
        match scenario_rc_opt {
            None => self.bc.broadcast(&UnitEvent::Log(LogEntry::new_error(sender_name.clone(), format!("unable to list tests, scenario {} not found", scenario_id)))),
            Some(ref sc_ref) => {
                let scenario = sc_ref.borrow();
                self.send_messages_to(sender_name, vec![ManagerStatusMessage::Tests(scenario.id().clone(), scenario.test_sequence())])
            }
        }
    }

    fn broadcast_selected_jig(&self) {
        let jig_opt = self.current_jig.borrow();
        match *jig_opt {
            None => return,
            Some(ref j) => {
                let jig = j.borrow();
                for (interface_id, _) in self.interfaces.borrow().iter() {
                    let messages = vec![
                        ManagerStatusMessage::Jig(Some(jig.id().clone()))
                    ];
                    self.send_messages_to(interface_id, messages);
                }
            }
        }
    }

    fn broadcast_scenario_list(&self) {
        let msg = ManagerStatusMessage::Scenarios(self.scenarios.borrow().keys().map(|x| x.clone()).collect());
        for (interface_id, _) in self.interfaces.borrow().iter() {
            self.send_messages_to(interface_id, vec![msg.clone()]);
        }
    }

    fn broadcast_selected_scenario(&self) {
        let opt = self.current_scenario.borrow();
        match *opt {
            None => return,
            Some(ref j) => {
                let scenario = j.borrow();
                for (interface_id, _) in self.interfaces.borrow().iter() {
                    let messages = vec![
                        ManagerStatusMessage::Scenario(Some(scenario.id().clone())),
                        ManagerStatusMessage::Tests(scenario.id().clone(), scenario.test_sequence())
                    ];
                    self.send_messages_to(interface_id, messages);
                }
            }
        }
    }

    fn broadcast_jig_named(&self, jig_id: &UnitName) {
        let jigs = self.jigs.borrow();
        let jig = match jigs.get(jig_id) {
            Some(ref s) => s.clone(),
            None => return,
        };
        for (interface_id, _) in self.interfaces.borrow().iter() {
            let jig = jig.borrow();
            let messages = vec![
                ManagerStatusMessage::Describe(jig.id().clone(), FieldType::Name, jig.name().clone()),
                ManagerStatusMessage::Describe(jig.id().clone(), FieldType::Description, jig.description().clone())
            ];
            self.send_messages_to(interface_id, messages);
        }
    }

    fn broadcast_scenario_named(&self, scenario_id: &UnitName) {
        let scenarios = self.scenarios.borrow();
        let scenario = match scenarios.get(scenario_id) {
            Some(ref s) => s.clone(),
            None => return,
        };

        self.broadcast_scenario_list();
        let messages = {
            let scenario = scenario.borrow();
            vec![
                // Rebroadcast the list of scenarios, since that may have changed.
                ManagerStatusMessage::Describe(scenario_id.clone(), FieldType::Name, scenario.name().clone()),
                ManagerStatusMessage::Describe(scenario_id.clone(), FieldType::Description, scenario.description().clone())
            ]
        };
        for (interface_id, _) in self.interfaces.borrow().iter() {
            self.send_messages_to(interface_id, messages.clone());
        }
    }

    fn broadcast_test_named(&self, unit_id: &UnitName) {
        let units = self.tests.borrow();
        let unit = match units.get(unit_id) {
            Some(ref s) => s.clone(),
            None => return,
        };
        for (interface_id, _) in self.interfaces.borrow().iter() {
            let unit = unit.borrow();
            let messages = vec![
                ManagerStatusMessage::Describe(unit_id.clone(), FieldType::Name, unit.name().clone()),
                ManagerStatusMessage::Describe(unit_id.clone(), FieldType::Description, unit.description().clone())
            ];
            self.send_messages_to(interface_id, messages);
        }
    }

    fn broadcast_skipped(&self, unit_id: &UnitName, reason: &String) {
        let msg = ManagerStatusMessage::Skipped(unit_id.clone(), reason.clone());
        for (interface_id, _) in self.interfaces.borrow().iter() {
            self.send_messages_to(interface_id, vec![msg.clone()]);
        }
    }

    fn broadcast_finished(&self, unit_id: &UnitName, code: u32, message: &String) {
        let msg = ManagerStatusMessage::Finished(unit_id.clone(), code, message.clone());
        for (interface_id, _) in self.interfaces.borrow().iter() {
            self.send_messages_to(interface_id, vec![msg.clone()]);
        }
    }

    fn broadcast_message(&self, msg: ManagerStatusMessage) {
        for (interface_id, _) in self.interfaces.borrow().iter() {
            self.send_messages_to(interface_id, vec![msg.clone()]);
        }
    }

    /// Send a Vec<ManagerStatusMessage> to a specific endpoint.
    pub fn send_messages_to(&self, sender_name: &UnitName, messages: Vec<ManagerStatusMessage>) {
        let mut deactivate_reason = None;
        match *sender_name.kind() {
            UnitKind::Interface => {
                let interface_table = self.interfaces.borrow();
                let interface = interface_table.get(sender_name).expect("Unable to find Interface in the library");
                for msg in messages {
                    if let Err(e) = interface.borrow().output_message(msg) {
                        deactivate_reason = Some(e);
                        break;
                    }
                }
            },
            _ => (),
        }
        if let Some(deactivate_reason) = deactivate_reason {
            self.deactivate(sender_name, format!("communication error: {}", deactivate_reason).as_str());
        }
    }
}