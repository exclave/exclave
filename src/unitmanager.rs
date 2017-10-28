use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use config::Config;
use unit::{UnitName, UnitKind};
use unitbroadcaster::{UnitBroadcaster, UnitEvent, UnitStatusEvent, LogEntry};
use units::interface::{Interface, InterfaceDescription};
use units::jig::{Jig, JigDescription};
use units::scenario::{Scenario, ScenarioDescription};
use units::test::{Test, TestDescription};

#[derive(Debug)]
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
#[derive(Debug)]
pub enum ManagerStatusMessage {
    /// Return the first name of the jig we're running on.
    Jig(UnitName /* Name of the jig */),

    /// Return a list of known scenarios.
    Scenarios(Vec<UnitName>),

    /// Return the currently-selected scenario, if any
    Scenario(Option<UnitName>),

    /// Return a list of tests in a scenario.
    Tests(UnitName /* Scenario name */, Vec<UnitName> /* List of tests */),

    /// Greeting identifying the server.
    Hello(String /* Server identification name */),

    /// Describes a Type of a particular Field on a given Unit
    Describe(UnitKind, FieldType, String /* UnitId */, String /* Value */),
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
    GetTests(Option<UnitName>),

    /// An error message from a particular interface.
    Error(String /* Error message contents */),

    /// Sent to a unit when it is first loaded, including "HELLO" messages.
    InitialGreeting,

    /// Indicates the child object terminated unexpectedly.
    ChildExited,

    /// Client sent an unimplemented message.
    Unimplemented(String /* verb */, String /* rest of line */),

    /// Send an INFO message to the logging system
    Log(String /* log message */),
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

    /// Loaded Interfaces, available for checkout.
    interfaces: RefCell<HashMap<UnitName, Interface>>,

    /// Loaded Jigs, available for checkout.
    jigs: RefCell<HashMap<UnitName, Arc<Mutex<Jig>>>>,

    /// Loaded Scenarios, available for checkout.
    scenarios: Rc<RefCell<HashMap<UnitName, Scenario>>>,

    /// Loaded Tests, available for checkout.
    tests: Rc<RefCell<HashMap<UnitName, Arc<Mutex<Test>>>>>,

    /// Prototypical message sender that will be cloned and passed to each new unit.
    control_sender: Sender<ManagerControlMessage>,

    /// The name of the currently-selected Scenario, if any
    current_scenario: Rc<RefCell<Option<UnitName>>>,

    /// The name of the currently-selected Jig, if any
    current_jig: Rc<RefCell<Option<UnitName>>>,
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

    pub fn select_interface(&self, description: &InterfaceDescription) -> Result<Interface, ()> {
        // If the interface exists in the array already, then it is active and will be deactivated first.
        if let Some(old_interface) = self.interfaces.borrow_mut().remove(description.id()) {
            match old_interface.deactivate() {
                Ok(_) =>
            self.bc.broadcast(
                    &UnitEvent::Status(UnitStatusEvent::new_deactivate_success(description.id(), "Reloading interface".to_owned()))),
                Err(e) =>
            self.bc.broadcast(
                    &UnitEvent::Status(UnitStatusEvent::new_deactivate_failure(description.id(), format!("Unable to deactivate: {}", e)))),
            }

            // After deactivating the old interface, deselect it.
            self.bc
                .broadcast(&UnitEvent::Status(UnitStatusEvent::new_deselected(description.id())));
        }

        // "Select" the Interface, which means we can activate it later on.
        let new_interface = match description.select(self, &*self.cfg.lock().unwrap()) {
            Ok(o) => o,
            Err(e) => {
                self.bc.broadcast(
                    &UnitEvent::Status(UnitStatusEvent::new_unit_incompatible(
                        description.id(),
                        format!("{}", e),
                    )),
                );
                return Err(());
            }
        };

        // Announce the fact that the interface was selected successfully.
        self.bc
            .broadcast(&UnitEvent::Status(UnitStatusEvent::new_selected(description.id())));
        Ok(new_interface)
    }

    pub fn activate_interface(&self, interface: Interface) {
        // Activate the interface, which actually starts it up.
        match interface.activate(self, &*self.cfg.lock().unwrap()) {
            Err(e) => {
            self.bc
            .broadcast(&UnitEvent::Status(UnitStatusEvent::new_active_failed(interface.id(), format!("{}", e))));
            return;
            },
            Ok(i) => i,
        };

        // Announce that the interface was successfully started.
        self.bc
            .broadcast(&UnitEvent::Status(UnitStatusEvent::new_active(interface.id())));

        self.interfaces
            .borrow_mut()
            .insert(interface.id().clone(),
                    interface);
    }


    pub fn select_jig(&self, description: &JigDescription) -> Result<Jig, ()> {
        self.jigs.borrow_mut().remove(description.id());

        // "Select" the Jig, which means we can activate it later on.
        let new_jig = match description.select(self, &*self.cfg.lock().unwrap()) {
            Ok(o) => o,
            Err(e) => {
                self.bc.broadcast(
                    &UnitEvent::Status(UnitStatusEvent::new_unit_incompatible(
                        description.id(),
                        format!("{}", e),
                    )),
                );
                return Err(());
            }
        };

        self.bc
            .broadcast(&UnitEvent::Status(UnitStatusEvent::new_selected(description.id())));
        Ok(new_jig)
    }

    /// Announce that the jig was successfully started.
    /// Only do so if there aren't any other valid, active jigs.
    pub fn activate_jig(&self, jig: Jig) {
        let mut current_jig = self.current_jig.borrow_mut();
        if let &Some(ref current_jig_name) = &*current_jig {
            // Don't continue if the jig is already loaded.
            if self.jigs.borrow().get(&current_jig_name).is_some() {
                return;
            }
            // Don't re-load the current jig.
            if current_jig_name == jig.id() {
                return;
            }
        }
        *current_jig = Some(jig.id().clone());
        *self.current_scenario.borrow_mut() = jig.default_scenario().clone();

        self.bc
            .broadcast(&UnitEvent::Status(UnitStatusEvent::new_active(jig.id())));

        self.jigs
            .borrow_mut()
            .insert(jig.id().clone(), Arc::new(Mutex::new(jig)));
    }

    pub fn load_test(&self, description: &TestDescription) {
        self.tests.borrow_mut().remove(description.id());

        // "Select" the Jig, which means we can activate it later on.
        let new_test = match description.select(self, &*self.cfg.lock().unwrap()) {
            Ok(o) => o,
            Err(e) => {
                self.bc.broadcast(
                    &UnitEvent::Status(UnitStatusEvent::new_unit_incompatible(
                        description.id(),
                        format!("{}", e),
                    )),
                );
                return;
            }
        };

        self.tests
            .borrow_mut()
            .insert(description.id().clone(), Arc::new(Mutex::new(new_test)));
        self.bc
            .broadcast(&UnitEvent::Status(UnitStatusEvent::new_selected(description.id())));
    }

    pub fn load_scenario(&self, description: &ScenarioDescription) {
        self.scenarios.borrow_mut().remove(description.id());

        // "Select" the Jig, which means we can activate it later on.
        let new_scenario = match description.select(self, &*self.cfg.lock().unwrap()) {
            Ok(o) => o,
            Err(e) => {
                self.bc.broadcast(
                    &UnitEvent::Status(UnitStatusEvent::new_unit_incompatible(
                        description.id(),
                        format!("{}", e),
                    )),
                );
                return;
            }
        };

        self.scenarios
            .borrow_mut()
            .insert(description.id().clone(), new_scenario);
        self.bc
            .broadcast(&UnitEvent::Status(UnitStatusEvent::new_selected(description.id())));
    }
    
    pub fn remove_interface(&self, id: &UnitName) {
        self.interfaces.borrow_mut().remove(id);
    }

    pub fn remove_jig(&self, id: &UnitName) {
        self.jigs.borrow_mut().remove(id);
    }

    pub fn remove_test(&self, id: &UnitName) {
        self.tests.borrow_mut().remove(id);
    }

    pub fn remove_scenario(&self, id: &UnitName) {
        self.scenarios.borrow_mut().remove(id);
    }

    pub fn jig_is_loaded(&self, id: &UnitName) -> bool {
        self.jigs.borrow().get(id).is_some()
    }

    pub fn get_test(&self, id: &UnitName) -> Option<Arc<Mutex<Test>>> {
        match self.tests.borrow().get(id) {
            None => None,
            Some(test) => Some(test.clone()),
        }
    }

    pub fn get_tests(&self) -> Rc<RefCell<HashMap<UnitName, Arc<Mutex<Test>>>>> {
        self.tests.clone()
    }

    pub fn get_scenarios(&self) -> Rc<RefCell<HashMap<UnitName, Scenario>>> {
        self.scenarios.clone()
    }

    pub fn process_message(&self, msg: &UnitEvent) {
        match msg {
            &UnitEvent::ManagerRequest(ref req) => self.manager_request(req),
            _ => (),
        }
    }

    fn manager_request(&self, msg: &ManagerControlMessage) {
        let &ManagerControlMessage {sender: ref sender_name, contents: ref msg} = msg;

        match *msg {
            ManagerControlMessageContents::Scenarios => self.send_scenarios(sender_name),
            ManagerControlMessageContents::GetTests(ref scenario_name) => self.send_tests(sender_name, scenario_name),
            ManagerControlMessageContents::Log(ref txt) => self.bc.broadcast(&UnitEvent::Log(LogEntry::new_info(sender_name.clone(), txt.clone()))),
            ManagerControlMessageContents::Scenario(ref new_scenario_name) => {
                if self.scenarios.borrow().get(new_scenario_name).is_some() {
                    *self.current_scenario.borrow_mut() = Some(new_scenario_name.clone());
                    self.send_scenario(sender_name, &Some(new_scenario_name.clone()));
                } else {
                    self.bc.broadcast(&UnitEvent::Log(LogEntry::new_error(sender_name.clone(), format!("unable to find scenario {}", new_scenario_name))));
                }
            },
            ManagerControlMessageContents::Error(ref err) => {
                self.bc.broadcast(&UnitEvent::Log(LogEntry::new_error(sender_name.clone(), err.clone())));
            },
            ManagerControlMessageContents::Jig => self.send_jig(sender_name),
            ManagerControlMessageContents::InitialGreeting => {
                // Send some initial information to the client.
                self.send_hello(sender_name);
                self.send_jig(sender_name);
                self.send_scenarios(sender_name);
                self.send_scenario(sender_name, &*self.current_scenario.borrow());
            },
            ManagerControlMessageContents::ChildExited => {
                self.bc.broadcast(&UnitEvent::Status(UnitStatusEvent::new_active_failed(sender_name, "Unit unexpectedly exited".to_owned())));
            }
            ManagerControlMessageContents::Unimplemented(ref verb, ref remainder) => {
                self.bc.broadcast(&UnitEvent::Log(LogEntry::new_error(sender_name.clone(), format!("unimplemented verb: {} (args: {})", verb, remainder))));
            },
        }
    }

    pub fn send_hello(&self, sender_name: &UnitName) {
        self.send_responses(sender_name, vec![ManagerStatusMessage::Hello("Jig/20 1.0".to_owned())]);
    }

    pub fn send_jig(&self, sender_name: &UnitName) {
        let messages = match *self.current_jig.borrow() {
            None => vec![ManagerStatusMessage::Jig(UnitName::from_str("", "jig").unwrap())],
            Some(ref jig_name) => {
                let jigs = self.jigs.borrow();
                // Unwrap, because the jig most definitely should exist.
                let jig = jigs.get(jig_name).unwrap().lock().unwrap();
                vec![
                    ManagerStatusMessage::Jig(jig.id().clone()),
                    ManagerStatusMessage::Describe(jig.id().kind().clone(), FieldType::Name, jig.id().id().clone(), jig.name().clone()),
                    ManagerStatusMessage::Describe(jig.id().kind().clone(), FieldType::Description, jig.id().id().clone(), jig.description().clone())
                ]
            }
        };
        self.send_responses(sender_name, messages);
    }

    pub fn send_scenarios(&self, sender_name: &UnitName) {
        let mut messages = vec![ManagerStatusMessage::Scenarios(self.scenarios.borrow().keys().map(|x| x.clone()).collect())];
        for (scenario_id, scenario) in self.scenarios.borrow().iter() {
            messages.push(ManagerStatusMessage::Describe(scenario_id.kind().clone(), FieldType::Name, scenario_id.id().clone(), scenario.name().clone()));
            messages.push(ManagerStatusMessage::Describe(scenario_id.kind().clone(), FieldType::Description, scenario_id.id().clone(), scenario.description().clone()));
        }
        self.send_responses(sender_name, messages);
    }

    pub fn send_scenario(&self, sender_name: &UnitName, scenario_name_opt: &Option<UnitName>) {
        let messages = if let &Some(ref scenario_name) = scenario_name_opt {
            if let Some(scenario) = self.scenarios.borrow().get(scenario_name) {
                let mut messages = vec![ManagerStatusMessage::Scenario(Some(scenario_name.clone()))];
                for (test_id, test_mtx) in scenario.tests() {
                    let test = test_mtx.lock().unwrap();
                    messages.push(ManagerStatusMessage::Describe(test_id.kind().clone(), FieldType::Name, test_id.id().clone(), test.name().clone()));
                    messages.push(ManagerStatusMessage::Describe(test_id.kind().clone(), FieldType::Description, test_id.id().clone(), test.description().clone()));
                }
                messages.push(ManagerStatusMessage::Tests(scenario.id().clone(), scenario.test_sequence()));
                messages
            } else {
                vec![ManagerStatusMessage::Scenario(None)]
            }
        } else {
            vec![ManagerStatusMessage::Scenario(None)]
        };
        self.send_responses(sender_name, messages);
    }

    pub fn send_tests(&self, sender_name: &UnitName, scenario_name_opt: &Option<UnitName>) {
        match *scenario_name_opt {
            None => if let Some(ref scenario_name) = *self.current_scenario.borrow() {
                let scenarios = self.scenarios.borrow();
                match scenarios.get(scenario_name) {
                    Some(ref scenario) => self.send_responses(sender_name, vec![ManagerStatusMessage::Tests(scenario.id().clone(), scenario.test_sequence())]),
                    None => {
                        self.bc.broadcast(&UnitEvent::Log(LogEntry::new_error(sender_name.clone(), format!("unable to list tests, default scenario {} not found", scenario_name))));
                    } 
                }
            } else {
                self.bc.broadcast(&UnitEvent::Log(LogEntry::new_error(sender_name.clone(), "unable to list tests, and no default scenario found".to_owned())));
            },
            Some(ref scenario_name) => {
                let scenarios = self.scenarios.borrow();
                match scenarios.get(scenario_name) {
                    Some(ref scenario) => self.send_responses(sender_name, vec![ManagerStatusMessage::Tests(scenario.id().clone(), scenario.test_sequence())]),
                    None => {
                        self.bc.broadcast(&UnitEvent::Log(LogEntry::new_error(sender_name.clone(), format!("unable to list tests, scenario {} not found", scenario_name))));
                    },
                }
            }
        }
    }

    pub fn send_responses(&self, sender_name: &UnitName, messages: Vec<ManagerStatusMessage>) {
        match *sender_name.kind() {
            UnitKind::Interface => {
                let interface_table = self.interfaces.borrow();
                let interface = interface_table.get(sender_name).expect("Unable to find Interface in the library");
                for msg in messages {
                    interface.output_message(msg).expect("Unable to pass message to client");
                }
            },
            _ => (),
        }
    }
}