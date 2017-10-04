// This UnitLibrary contains all active, loaded modules.
// When

use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::cell::RefCell;

use config::Config;
use unit::{UnitName, UnitKind};
use unitbroadcaster::{UnitBroadcaster, UnitEvent, UnitStatus, UnitStatusEvent, UnitCategoryEvent};
use units::jig::JigDescription;
use units::test::TestDescription;
use units::scenario::ScenarioDescription;


pub struct UnitLibrary {
    broadcaster: UnitBroadcaster,
    receiver: Receiver<UnitEvent>,
    config: Arc<Mutex<Config>>,

    /// The unit status is used to determine whether to reload units or not.
    unit_status: RefCell<HashMap<UnitName, UnitStatus>>,

    /// Currently available jig descriptions.  May not be valid.
    jig_descriptions: RefCell<HashMap<UnitName, JigDescription>>,

    /// Currently available test descriptions.  The tests they describe may not be valid.
    test_descriptions: RefCell<HashMap<UnitName, TestDescription>>,

    /// Currently available scenario descriptions.  The scenarios they describe may not be valid.
    scenario_descriptions: RefCell<HashMap<UnitName, ScenarioDescription>>,

    /// A list of jig names that must be checked when a rescan() is performed.
    dirty_jigs: RefCell<HashMap<UnitName, ()>>,
    dirty_tests: RefCell<HashMap<UnitName, ()>>,
    dirty_scenarios: RefCell<HashMap<UnitName, ()>>,
}

impl UnitLibrary {
    pub fn new(broadcaster: &UnitBroadcaster, config: &Arc<Mutex<Config>>) -> Self {

        UnitLibrary {
            broadcaster: broadcaster.clone(),
            receiver: broadcaster.subscribe(),
            config: config.clone(),
            unit_status: RefCell::new(HashMap::new()),
            jig_descriptions: RefCell::new(HashMap::new()),
            test_descriptions: RefCell::new(HashMap::new()),
            scenario_descriptions: RefCell::new(HashMap::new()),
            dirty_jigs: RefCell::new(HashMap::new()),
            dirty_tests: RefCell::new(HashMap::new()),
            dirty_scenarios: RefCell::new(HashMap::new()),
        }
    }

    pub fn update_jig_description(&mut self, description: JigDescription) {

        let id = description.id().clone();

        // Add the jig name to a list of "dirty jigs" that will be checked during "rescan()"
        self.dirty_jigs.borrow_mut().insert(id.clone(), ());

        // Add an entry to the status to determine whether this unit is new or not.
        match self.jig_descriptions.borrow_mut().insert(description.id().clone(), description) {
            None => self.unit_status.borrow_mut().insert(id.clone(), UnitStatus::LoadStarted),
            Some(_) => self.unit_status.borrow_mut().insert(id.clone(), UnitStatus::UpdateStarted),
        };

        self.broadcaster
            .broadcast(&UnitEvent::Category(UnitCategoryEvent::new(UnitKind::Jig,
                                                                   &format!("Number of units \
                                                                             loaded: {}",
                                                                            self.jig_descriptions
                                                                                .borrow()
                                                                                .len()))));
    }

    pub fn update_test_description(&mut self, test_description: TestDescription) {

        // Notify everyone this unit has been selected.
        self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_load_started(&test_description.id())));

        self.test_descriptions.borrow_mut().insert(test_description.id().clone(), test_description);

        self.broadcaster
            .broadcast(&UnitEvent::Category(UnitCategoryEvent::new(UnitKind::Test,
                                                                   &format!("Number of tests \
                                                                             loaded: {}",
                                                                            self.test_descriptions
                                                                                .borrow()
                                                                                .len()))));
    }

    pub fn update_scenario_description(&mut self, scenario_description: ScenarioDescription) {

        // Notify everyone this unit has been selected.
        self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_load_started(&scenario_description.id())));

        self.scenario_descriptions
            .borrow_mut()
            .insert(scenario_description.id().clone(), scenario_description);

        self.broadcaster
            .broadcast(&UnitEvent::Category(UnitCategoryEvent::new(UnitKind::Scenario,
                                                                   &format!("Number of scenarios \
                                                                             loaded: {}",
                                                                            self.scenario_descriptions
                                                                                .borrow()
                                                                                .len()))));
    }

    pub fn remove_jig(&mut self, jig_name: &UnitName) {
        self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_unloading(&jig_name)));
        self.jig_descriptions.borrow_mut().remove(jig_name);
    }

    pub fn remove_test(&mut self, test_name: &UnitName) {
        self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_unloading(&test_name)));
        self.test_descriptions.borrow_mut().remove(test_name);
    }

    pub fn remove_scenario(&mut self, scenario_name: &UnitName) {
        self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_unloading(&scenario_name)));
        self.scenario_descriptions.borrow_mut().remove(scenario_name);
    }

    /// Examine all of the loaded units and ensure they can be loaded.
    ///
    /// Start by updating the list of "dirty" items that depend on it:
    ///  * Jig: Tests and Scenarios
    ///  * Scenarios: Tests
    ///
    /// Next, delete all "dirty" objects that are Deleted.
    ///
    /// Finally, go through all "dirty" objects and configure them:
    ///  * Jigs
    ///  * Tests
    ///  * Scenarios
    /// Where "configure" means check if it's compatible:
    ///  * If it's compatible and unloaded, then load it
    ///  * If it's incompatible and loaded, then unload it
    ///  * If it's compatible and loaded, then do nothing
    ///  * If it's incompatible and unloaded, then do nothing
    pub fn rescan(&mut self) {
        self.broadcaster.broadcast(&UnitEvent::RescanStart);

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
                    self.dirty_scenarios.borrow_mut().insert(scenario_name.clone(), ());
                }
            }

            // XXX Do something different depending on the Status
            self.load_jig(jig_name);
        }

        self.broadcaster.broadcast(&UnitEvent::RescanFinish);
    }

    fn load_jig(&self, name: &UnitName) {
        assert_eq!(name.kind(), &UnitKind::Jig);

        let jig_descriptions = self.jig_descriptions.borrow();

        // Get the description.
        // It is very much an error if this function is called with an invalid name.
        let description = jig_descriptions.get(name).unwrap();

        // Check to see if the jig is compatible with this platform
        if let Err(e) = description.is_compatible(self, &*self.config.lock().unwrap()) {
            self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_unit_incompatible(name, format!("{}", e))));
            return;
        }

        // "Select" the Jig, which means we can activate it later on.
        let new_jig = match description.select() {
            Ok(o) => o,
            Err(e) => {
                self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_select_failed(name, format!("{}", e))));
                return;
            }
        };
    }
}