// This UnitLibrary contains all active, loaded modules.
// When

use std::path::PathBuf;
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
    jig_descriptions: RefCell<HashMap<UnitName, Arc<Mutex<JigDescription>>>>,
    test_descriptions: RefCell<HashMap<UnitName, Arc<Mutex<TestDescription>>>>,
    scenario_descriptions: RefCell<HashMap<UnitName, Arc<Mutex<ScenarioDescription>>>>,
}

impl UnitLibrary {
    pub fn new(broadcaster: &UnitBroadcaster, config: &Arc<Mutex<Config>>) -> Self {

        UnitLibrary {
            broadcaster: broadcaster.clone(),
            receiver: broadcaster.subscribe(),
            config: config.clone(),
            jig_descriptions: RefCell::new(HashMap::new()),
            test_descriptions: RefCell::new(HashMap::new()),
            scenario_descriptions: RefCell::new(HashMap::new()),
        }
    }

    pub fn update_jig_description(&mut self, jig_description: JigDescription) {
        // Notify everyone this unit has been selected.
        self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_load_started(&jig_description.id())));

        self.jig_descriptions.borrow_mut().insert(jig_description.id().clone(), Arc::new(Mutex::new(jig_description)));
/*
                // Check to see if the jig is compatible with this platform
                if let Err(e) = jig_description.is_compatible(&*self.config.lock().unwrap()) {
                    self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_unit_incompatible(name, format!("{}", e))));
                    return;
                }

                // "Select" the Jig, which means we can activate it later on.
                let new_jig = match jig_description.select() {
                    Ok(o) => o,
                    Err(e) => {
                        self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_select_failed(name, format!("{}", e))));
                        return;
                    }
                };
*/

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

        self.test_descriptions.borrow_mut().insert(test_description.id().clone(), Arc::new(Mutex::new(test_description)));

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

        self.scenario_descriptions.borrow_mut().insert(scenario_description.id().clone(), Arc::new(Mutex::new(scenario_description)));

        self.broadcaster
            .broadcast(&UnitEvent::Category(UnitCategoryEvent::new(UnitKind::Scenario,
                                                                   &format!("Number of scenarios \
                                                                             loaded: {}",
                                                                            self.scenario_descriptions
                                                                                .borrow()
                                                                                .len()))));
    }

    pub fn remove_jig(&mut self, jig_name: &UnitName) {
        self.jig_descriptions.borrow_mut().remove(jig_name);
    }

    pub fn remove_test(&mut self, test_name: &UnitName) {
        self.test_descriptions.borrow_mut().remove(test_name);
    }

    pub fn remove_scenario(&mut self, scenario_name: &UnitName) {
        self.scenario_descriptions.borrow_mut().remove(scenario_name);
    }

    /// Examine all of the loaded units and ensure they can be loaded.
    pub fn rescan(&mut self) {
        
    }
}