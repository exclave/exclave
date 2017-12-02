// The UnitLibrary contains plans to load each valid Unit.  Units may
// not actually be selected, e.g. if they aren't compatible.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use config::Config;
use unit::{UnitKind, UnitName};
use unitbroadcaster::{UnitBroadcaster, UnitCategoryEvent, UnitEvent, UnitStatus, UnitStatusEvent};
use unitmanager::UnitManager;
use units::interface::InterfaceDescription;
use units::jig::{JigDescription};
use units::logger::LoggerDescription;
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

                    // Insert it into the description table
                    $slf.$desc.borrow_mut().insert(id.clone(), description);

                    // Add the unit name to a list of "dirty units" that will be checked during "rescan()"
                    $slf.$drty.borrow_mut().insert(id.clone(), ());

                    // Add an entry to the status to determine whether this unit is new or not.
                    $slf.unit_status
                        .borrow_mut()
                        .insert(id, $status.clone());

                    $slf.broadcaster
                        .broadcast(&UnitEvent::Category(UnitCategoryEvent::new($tstkind,
                                                                            &format!(
                                "Number of units \
                                on disk: {}",
                                $slf.$desc.borrow().len()
                            ))));
                }
            }
        }
    }
}

macro_rules! load_units_for_activation {
    ($slf:ident, $statuses:ident, $dirty:ident, $descriptions:ident, $load:ident) => {
        {
            let mut to_remove = vec![];
            for (id, _) in $slf.$dirty.borrow().iter() {
                let status = $statuses.get(id);
                if status.is_none() {
                    to_remove.push(id.clone());
                    continue;
                }
                let status = status.unwrap();

                let descriptions = $slf.$descriptions.borrow();
                let description = descriptions.get(id);
                if description.is_none() {
                    to_remove.push(id.clone());
                    continue;
                }
                let description = description.unwrap();

                let load_result = match status {
                    &UnitStatus::LoadStarted(_) => $slf.unit_manager.borrow_mut().$load(description),
                    &UnitStatus::UpdateStarted(_) => $slf.unit_manager.borrow_mut().$load(description),
                    x => panic!("Unexpected unit status: {}", x),
                };

                if load_result.is_err() {
                    to_remove.push(id.clone());
                }
            }
            let mut dirty = $slf.$dirty.borrow_mut();
            for id in to_remove {
                dirty.remove(&id);
            }
        }
    }
}

macro_rules! select_and_activate_units {
    ($slf:ident, $dirty:ident) => {
        {
            for (id, _) in $slf.$dirty.borrow().iter() {
                $slf.unit_manager.borrow_mut().select(id);
                $slf.unit_manager.borrow_mut().activate(id);
            }
            $slf.$dirty.borrow_mut().clear();
        }
    }
}

macro_rules! load_units {
    ($slf:ident, $statuses:ident, $dirty:ident, $descriptions:ident, $load:ident) => {
        load_units_for_activation!($slf, $statuses, $dirty, $descriptions, $load);
        $slf.$dirty.borrow_mut().clear();
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

    /// Currently available logger descriptions.
    logger_descriptions: RefCell<HashMap<UnitName, LoggerDescription>>,

    /// Currently available scenario descriptions.  The scenarios they describe might not be valid.
    scenario_descriptions: RefCell<HashMap<UnitName, ScenarioDescription>>,

    /// Currently available test descriptions.  The tests they describe might not be valid.
    test_descriptions: RefCell<HashMap<UnitName, TestDescription>>,

    /// A list of unit names that must be checked when a rescan() is performed.
    dirty_interfaces: RefCell<HashMap<UnitName, ()>>,
    dirty_jigs: RefCell<HashMap<UnitName, ()>>,
    dirty_loggers: RefCell<HashMap<UnitName, ()>>,
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
            logger_descriptions: RefCell::new(HashMap::new()),
            scenario_descriptions: RefCell::new(HashMap::new()),
            test_descriptions: RefCell::new(HashMap::new()),

            dirty_interfaces: RefCell::new(HashMap::new()),
            dirty_jigs: RefCell::new(HashMap::new()),
            dirty_loggers: RefCell::new(HashMap::new()),
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
    /// 4. Select all Jigs that are valid.
    /// 5. Select all Interfaces that are valid.
    /// 6. Select all Tests that are compatible with this Jig.
    /// 7. Select all Scenarios.
    /// 8. Activate all Jigs (only the last one will be 'active')
    /// 9. Activate all Interfaces.
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

            for (logger_name, logger_description) in self.logger_descriptions
                .borrow()
                .iter() {
                if logger_description.supports_jig(jig_name) {
                    self.dirty_loggers.borrow_mut().insert(logger_name.clone(), ());
                }
            }
        }

        // 2. Go through tests and mark scenarios as dirty.
        for (test_name, _) in self.dirty_tests.borrow().iter() {
            let unit_manager = self.unit_manager.borrow();
            let scenarios_rc = unit_manager.get_scenarios();
            let scenarios = scenarios_rc.borrow();
            for (scenario_name, scenario) in scenarios.iter() {
                if scenario.borrow().uses_test(test_name) {
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
                self.unit_manager.borrow_mut().unload(id);
                statuses.remove(id);
            }
        }
        for (id, _) in self.dirty_tests.borrow().iter() {
            if let &UnitStatus::UnloadStarted(_) = statuses.get(id).expect("Unable to find status in dirty test list") {
                self.test_descriptions.borrow_mut().remove(id);
                self.unit_manager.borrow_mut().unload(id);
                statuses.remove(id);
            }
        }
        for (id, _) in self.dirty_scenarios.borrow().iter() {
            if let &UnitStatus::UnloadStarted(_) = statuses.get(id).expect("Unable to find status in dirty scenario list") {
                self.scenario_descriptions.borrow_mut().remove(id);
                self.unit_manager.borrow_mut().unload(id);
                statuses.remove(id);
            }
        }
        for (id, _) in self.dirty_interfaces.borrow().iter() {
            if let &UnitStatus::UnloadStarted(_) = statuses.get(id).expect("Unable to find status in dirty interface list") {
                self.interface_descriptions.borrow_mut().remove(id);
                self.unit_manager.borrow_mut().unload(id);
                statuses.remove(id);
            }
        }
        for (id, _) in self.dirty_loggers.borrow().iter() {
            if let &UnitStatus::UnloadStarted(_) = statuses.get(id).expect("Unable to find status in dirty logger list") {
                self.logger_descriptions.borrow_mut().remove(id);
                self.unit_manager.borrow_mut().unload(id);
                statuses.remove(id);
            }
        }

        // 4. Load all Jigs that are valid.
        load_units_for_activation!(self, statuses, dirty_jigs, jig_descriptions, load_jig);

        // 5. Load all Interfaces that are compatible with this Jig.
        load_units_for_activation!(self, statuses, dirty_interfaces, interface_descriptions, load_interface);

        // 6. Load all loggers that are compatible with this Jig.
        load_units_for_activation!(self, statuses, dirty_loggers, logger_descriptions, load_logger);

        // 7. Load all Tests that are compatible with this Jig.
        load_units!(self, statuses, dirty_tests, test_descriptions, load_test);

        // 8. Load all Scenarios that are compatible with this Jig.
        load_units!(self, statuses, dirty_scenarios, scenario_descriptions, load_scenario);

        // 9. Activate all jigs that were just loaded.
        select_and_activate_units!(self, dirty_jigs);

        // 10. Activate all interfaces that were just loaded.
        select_and_activate_units!(self, dirty_interfaces);

        // 11. Activate all loggers that were just loaded.
        select_and_activate_units!(self, dirty_loggers);

        // 12. Prepare any defaults that need loading (i.e. jigs, scenarios, etc.)
        self.unit_manager.borrow_mut().refresh_defaults();

        self.broadcaster.broadcast(&UnitEvent::RescanFinish);
    }

    pub fn process_message(&mut self, evt: &UnitEvent) {
        match evt {
            &UnitEvent::Status(ref msg) =>  {
                let &UnitStatusEvent {ref name, ref status} = msg;

                match status {
                    &UnitStatus::LoadStarted(ref path) => {
                        process_if!(self, name, status, UnitKind::Interface, path, InterfaceDescription, dirty_interfaces, interface_descriptions);
                        process_if!(self, name, status, UnitKind::Jig, path, JigDescription, dirty_jigs, jig_descriptions);
                        process_if!(self, name, status, UnitKind::Logger, path, LoggerDescription, dirty_loggers, logger_descriptions);
                        process_if!(self, name, status, UnitKind::Test, path, TestDescription, dirty_tests, test_descriptions);
                        process_if!(self, name, status, UnitKind::Scenario, path, ScenarioDescription, dirty_scenarios, scenario_descriptions);
                    }
                    &UnitStatus::UpdateStarted(ref path) => {
                        process_if!(self, name, status, UnitKind::Interface, path, InterfaceDescription, dirty_interfaces, interface_descriptions);
                        process_if!(self, name, status, UnitKind::Jig, path, JigDescription, dirty_jigs, jig_descriptions);
                        process_if!(self, name, status, UnitKind::Logger, path, LoggerDescription, dirty_loggers, logger_descriptions);
                        process_if!(self, name, status, UnitKind::Scenario, path, ScenarioDescription, dirty_scenarios, scenario_descriptions);
                        process_if!(self, name, status, UnitKind::Test, path, TestDescription, dirty_tests, test_descriptions);
                    }
                    &UnitStatus::UnloadStarted(ref path) => {
                        // Add the unit name to a list of "dirty units" that will be checked during "rescan()"
                        self.unit_status
                            .borrow_mut()
                            .insert(name.clone(), UnitStatus::UnloadStarted(path.clone()));
                        match name.kind() {
                            &UnitKind::Interface => self.dirty_interfaces.borrow_mut().insert(name.clone(), ()),
                            &UnitKind::Jig => self.dirty_jigs.borrow_mut().insert(name.clone(), ()),
                            &UnitKind::Logger => self.dirty_loggers.borrow_mut().insert(name.clone(), ()),
                            &UnitKind::Test => self.dirty_tests.borrow_mut().insert(name.clone(), ()),
                            &UnitKind::Scenario => self.dirty_scenarios.borrow_mut().insert(name.clone(), ()),
                            &UnitKind::Internal => None,
                        };
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
