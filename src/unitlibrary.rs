// This UnitLibrary contains all active, loaded modules, as well as the
// "descriptions" that can be used to [re]load modules.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use config::Config;
use unit::{UnitKind, UnitName};
use unitbroadcaster::{UnitBroadcaster, UnitCategoryEvent, UnitEvent, UnitStatus, UnitStatusEvent};
use unitmanager::UnitManager;
use units::interface::InterfaceDescription;
use units::jig::{JigDescription};
use units::scenario::{ScenarioDescription};
use units::test::{TestDescription};

macro_rules! process_if {
    ($slf:ident, $name:ident, $status:ident, $tstkind:path, $path:ident, $trgt:ident, $drty:ident, $desc:ident) => {
        if $name.kind() == &$tstkind {
            match $trgt::from_path($path) {
                Err(e) =>
                    $slf.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_load_failed($name, format!("{}", e)))),
                Ok(description) => {
                    let id = description.id().clone();

                    // Add the jig name to a list of "dirty jigs" that will be checked during "rescan()"
                    $slf.$drty.borrow_mut().insert(id.clone(), ());

                    // Add an entry to the status to determine whether this unit is new or not.
                    $slf.unit_status
                        .borrow_mut()
                        .insert(id.clone(), $status.clone());

                    // Insert it into the description table
                    $slf.$desc.borrow_mut().insert(id, description);

                    // Since the unit was loaded successfully, mark it as "Selected".
                    $slf.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_selected($name)));

                    $slf.broadcaster
                        .broadcast(&UnitEvent::Category(UnitCategoryEvent::new($tstkind,
                                                                            &format!(
                                "Number of units \
                                loaded: {}",
                                $slf.jig_descriptions.borrow().len()
                            ))));
                }
            }
        }
    }
}

pub struct UnitLibrary {
    broadcaster: UnitBroadcaster,

    /// The unit status is used to determine whether to reload units or not.
    unit_status: RefCell<HashMap<UnitName, UnitStatus>>,

    /// Currently available interface descriptions.  The interfaces they describe might not be valid.
    interface_descriptions: RefCell<HashMap<UnitName, InterfaceDescription>>,

    /// Currently available jig descriptions.  The jigs they describe might not be valid.
    jig_descriptions: RefCell<HashMap<UnitName, JigDescription>>,

    /// Currently available scenario descriptions.  The scenarios they describe might not be valid.
    scenario_descriptions: RefCell<HashMap<UnitName, ScenarioDescription>>,

    /// Currently available test descriptions.  The tests they describe might not be valid.
    test_descriptions: RefCell<HashMap<UnitName, TestDescription>>,

    /// A list of unit names that must be checked when a rescan() is performed.
    dirty_interfaces: RefCell<HashMap<UnitName, ()>>,
    dirty_jigs: RefCell<HashMap<UnitName, ()>>,
    dirty_scenarios: RefCell<HashMap<UnitName, ()>>,
    dirty_tests: RefCell<HashMap<UnitName, ()>>,

    /// The object in charge of keeping track of units in-memory.
    unit_manager: RefCell<UnitManager>,
}

impl UnitLibrary {
    pub fn new(broadcaster: &UnitBroadcaster, config: &Arc<Mutex<Config>>) -> Self {
        UnitLibrary {
            broadcaster: broadcaster.clone(),
            unit_status: RefCell::new(HashMap::new()),

            interface_descriptions: RefCell::new(HashMap::new()),
            jig_descriptions: RefCell::new(HashMap::new()),
            scenario_descriptions: RefCell::new(HashMap::new()),
            test_descriptions: RefCell::new(HashMap::new()),

            dirty_interfaces: RefCell::new(HashMap::new()),
            dirty_jigs: RefCell::new(HashMap::new()),
            dirty_scenarios: RefCell::new(HashMap::new()),
            dirty_tests: RefCell::new(HashMap::new()),

            unit_manager: RefCell::new(UnitManager::new(broadcaster, config)),
        }
    }

    /// Examine all of the loaded units and ensure they can be loaded.
    ///
    /// Each unit type must be handled differently.
    ///
    /// 1. Mark every Interface, Scenario or Test that depends on a dirty jig as dirty.
    ///    That way, they will be rescanned.
    /// 2. Mark every Scenario that uses a dirty Test as dirty.
    ///    That way, scenario dependency graphs will be re-evaluated.
    /// 3. Delete any "dirty" objects that were Deleted.
    /// 4. Load all Jigs that are valid.
    /// 5. Load all Interfaces that are valid.
    /// 6. Load all Tests that are compatible with this Jig.
    /// 7. Load all Scenarios.
    pub fn rescan(&mut self) {
        self.broadcaster.broadcast(&UnitEvent::RescanStart);
        let mut statuses = self.unit_status.borrow_mut();

        // 1. Go through jigs and mark dependent scenarios and tests as dirty.
        for (jig_name, _) in self.dirty_jigs.borrow().iter() {
            for (test_name, test_description) in self.test_descriptions.borrow().iter() {
                if test_description.supports_jig(jig_name) {
                    self.dirty_tests.borrow_mut().insert(test_name.clone(), ());
                }
            }

            for (scenario_name, scenario_description) in self.scenario_descriptions
                .borrow()
                .iter() {
                if scenario_description.supports_jig(jig_name) {
                    self.dirty_scenarios
                        .borrow_mut()
                        .insert(scenario_name.clone(), ());
                }
            }

            for (interface_name, interface_description) in self.interface_descriptions
                .borrow()
                .iter() {
                if interface_description.supports_jig(jig_name) {
                    self.dirty_interfaces.borrow_mut().insert(interface_name.clone(), ());
                }
            }
        }

        // 2. Go through tests and mark scenarios as dirty.
        for (test_name, _) in self.dirty_tests.borrow().iter() {
            let unit_manager = self.unit_manager.borrow();
            let scenarios_rc = unit_manager.get_scenarios();
            let scenarios = scenarios_rc.borrow();
            for (scenario_name, scenario) in scenarios.iter() {
                if scenario.lock().unwrap().uses_test(test_name) {
                    self.dirty_scenarios
                        .borrow_mut()
                        .insert(scenario_name.clone(), ());
                }
            }
        }

        // 3. Delete any "dirty" objects that were Deleted.
        for (id, _) in self.dirty_jigs.borrow().iter() {
            if let &UnitStatus::UnloadStarted(_) = statuses.get(id).unwrap() {
                self.jig_descriptions.borrow_mut().remove(id);
                self.unit_manager.borrow_mut().remove_jig(id);
                statuses.remove(id);
            }
        }
        for (id, _) in self.dirty_tests.borrow().iter() {
            if let &UnitStatus::UnloadStarted(_) = statuses.get(id).expect("Unable to find status in dirty test list") {
                self.test_descriptions.borrow_mut().remove(id);
                self.unit_manager.borrow_mut().remove_test(id);
                statuses.remove(id);
            }
        }
        for (id, _) in self.dirty_scenarios.borrow().iter() {
            if let &UnitStatus::UnloadStarted(_) = statuses.get(id).expect("Unable to find status in dirty scenario list") {
                self.scenario_descriptions.borrow_mut().remove(id);
                self.unit_manager.borrow_mut().remove_scenario(id);
                statuses.remove(id);
            }
        }
        for (id, _) in self.dirty_interfaces.borrow().iter() {
            if let &UnitStatus::UnloadStarted(_) = statuses.get(id).expect("Unable to find status in dirty interface list") {
                self.interface_descriptions.borrow_mut().remove(id);
                self.unit_manager.borrow_mut().remove_interface(id);
                statuses.remove(id);
            }
        }

        // 4. Load all Jigs that are valid.
        for (id, _) in self.dirty_jigs.borrow().iter() {
            match statuses.get(id).unwrap() {
                &UnitStatus::LoadStarted(_) => {
                    self.unit_manager.borrow_mut().load_jig(self.jig_descriptions.borrow().get(id).unwrap())
                }
                &UnitStatus::UpdateStarted(_) => {
                    self.unit_manager.borrow_mut().load_jig(self.jig_descriptions.borrow().get(id).unwrap())
                }
                x => panic!("Unexpected jig unit status: {}", x),
            }
        }
        self.dirty_jigs.borrow_mut().clear();

        // 5. Load all Interfaces that are compatible with this Jig.
        for (id, _) in self.dirty_interfaces.borrow().iter() {
        
            match statuses.get(id).unwrap() {
                &UnitStatus::LoadStarted(_) => {
                    self.unit_manager.borrow_mut().load_interface(self.interface_descriptions.borrow().get(id).unwrap())
                }
                &UnitStatus::UpdateStarted(_) => {
                    eprintln!("Updating interface in manager: {}", id);
                    self.unit_manager.borrow_mut().load_interface(self.interface_descriptions.borrow().get(id).unwrap())
                }
                x => panic!("Unexpected interface unit status: {}", x),
            }
        }
        self.dirty_interfaces.borrow_mut().clear();

        // 6. Load all Tests that are compatible with this Jig.
        for (id, _) in self.dirty_tests.borrow().iter() {
            match statuses.get(id).unwrap() {
                &UnitStatus::LoadStarted(_) => {
                    self.unit_manager.borrow_mut().load_test(self.test_descriptions.borrow().get(id).expect("Test status is present, but test is not in the description table"))
                }
                &UnitStatus::UpdateStarted(_) => {
                    self.unit_manager.borrow_mut().load_test(self.test_descriptions.borrow().get(id).expect("Test status is present, but test is not in the description table"))
                }
                x => panic!("Unexpected test unit status: {}", x),
            }
        }
        self.dirty_tests.borrow_mut().clear();

        // 7. Load all Scenarios that are compatible with this Jig.
        for (id, _) in self.dirty_scenarios.borrow().iter() {
            match statuses.get(id).unwrap() {
                &UnitStatus::LoadStarted(_) => {
                    self.unit_manager.borrow_mut().load_scenario(self.scenario_descriptions.borrow().get(id).unwrap())
                }
                &UnitStatus::UpdateStarted(_) => {
                    self.unit_manager.borrow_mut().load_scenario(self.scenario_descriptions.borrow().get(id).unwrap())
                }
                x => panic!("Unexpected scenario unit status: {}", x),
            }
        }
        self.dirty_scenarios.borrow_mut().clear();

        self.broadcaster.broadcast(&UnitEvent::RescanFinish);
    }

    pub fn process_message(&mut self, evt: &UnitEvent) {
        match evt {
            &UnitEvent::Status(ref msg) =>  {
                let &UnitStatusEvent {ref name, ref status} = msg;

                match status {
                    &UnitStatus::LoadStarted(ref path) => {
                        process_if!(self, name, status, UnitKind::Jig, path, JigDescription, dirty_jigs, jig_descriptions);
                        process_if!(self, name, status, UnitKind::Interface, path, InterfaceDescription, dirty_interfaces, interface_descriptions);
                        process_if!(self, name, status, UnitKind::Test, path, TestDescription, dirty_tests, test_descriptions);
                        process_if!(self, name, status, UnitKind::Scenario, path, ScenarioDescription, dirty_scenarios, scenario_descriptions);
                    }
                    &UnitStatus::UpdateStarted(ref path) => {
                        process_if!(self, name, status, UnitKind::Jig, path, JigDescription, dirty_jigs, jig_descriptions);
                        process_if!(self, name, status, UnitKind::Interface, path, InterfaceDescription, dirty_interfaces, interface_descriptions);
                        process_if!(self, name, status, UnitKind::Test, path, TestDescription, dirty_tests, test_descriptions);
                        process_if!(self, name, status, UnitKind::Scenario, path, ScenarioDescription, dirty_scenarios, scenario_descriptions);
                    }
                    &UnitStatus::UnloadStarted(ref path) => {
                        self.unit_status
                            .borrow_mut()
                            .insert(name.clone(), UnitStatus::UnloadStarted(path.clone()));
                    },
                    _ => (),
                }
            },
            &UnitEvent::RescanRequest => self.rescan(),
            _ => (),
        }

        // Also pass the message on to the unit manager.
        self.unit_manager.borrow().process_message(evt);
    }
}
