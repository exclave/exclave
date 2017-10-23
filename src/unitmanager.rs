use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use config::Config;
use unit::{UnitName, UnitKind};
use unitbroadcaster::{UnitBroadcaster, UnitEvent, UnitStatusEvent};
use units::interface::{Interface, InterfaceDescription};
use units::jig::{Jig, JigDescription};
use units::scenario::{Scenario, ScenarioDescription};
use units::test::{Test, TestDescription};

/// Messages for Library -> Unit communication
pub enum ManagerStatusMessage {
    /// Return the first name of the jig we're running on.
    Jig(UnitName /* Name of the jig */),

    /// Return a list of known scenarios.
    Scenarios(Vec<UnitName>),
}

/// Messages for Unit -> Library communication
#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub enum ManagerControlMessageContents {
    /// Get a list of compatible, Selected scenarios.
    Scenarios,

    /// Client sent an unimplemented message.
    Unimplemented(String /* verb */, String /* rest of line */),
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
    scenarios: Rc<RefCell<HashMap<UnitName, Arc<Mutex<Scenario>>>>>,

    /// Loaded Tests, available for checkout.
    tests: Rc<RefCell<HashMap<UnitName, Arc<Mutex<Test>>>>>,

    /// Prototypical message sender that will be cloned and passed to each new unit.
    control_sender: Sender<ManagerControlMessage>,
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

    pub fn load_interface(&self, description: &InterfaceDescription) {
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
                return;
            }
        };

        // Announce the fact that the interface was selected successfully.
        self.bc
            .broadcast(&UnitEvent::Status(UnitStatusEvent::new_selected(description.id())));

        match new_interface.activate(self, &*self.cfg.lock().unwrap()) {
            Err(e) => {
            self.bc
            .broadcast(&UnitEvent::Status(UnitStatusEvent::new_active_failed(description.id(), format!("{}", e))));
            return;
            },
            Ok(i) => i,
        };

        // Announce that the interface was successfully started.
        self.bc
            .broadcast(&UnitEvent::Status(UnitStatusEvent::new_active(description.id())));

        self.interfaces
            .borrow_mut()
            .insert(description.id().clone(),
                    new_interface);
    }


    pub fn load_jig(&self, description: &JigDescription) {
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
                return;
            }
        };
        self.jigs
            .borrow_mut()
            .insert(description.id().clone(), Arc::new(Mutex::new(new_jig)));
        self.bc
            .broadcast(&UnitEvent::Status(UnitStatusEvent::new_selected(description.id())));
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
            .insert(description.id().clone(), Arc::new(Mutex::new(new_scenario)));
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

    pub fn get_scenarios(&self) -> Rc<RefCell<HashMap<UnitName, Arc<Mutex<Scenario>>>>> {
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

        let response = match *msg {
            ManagerControlMessageContents::Scenarios => ManagerStatusMessage::Scenarios(self.scenarios.borrow().keys().map(|x| x.clone()).collect()),
            ManagerControlMessageContents::Unimplemented(ref verb, ref remainder) => { eprintln!("Unimplemented verb: {} {}", verb, remainder); return; },
        };

        match *sender_name.kind() {
            UnitKind::Interface => 
                self.interfaces.borrow().get(sender_name).expect("Unable to find Interface in the library").output_message(response),
            _ => Ok(()),
        }.expect("Unable to pass message to client");
    }
}