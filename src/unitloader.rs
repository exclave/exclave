use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use config::Config;
use unit::{UnitName, UnitKind};
use unitbroadcaster::{UnitBroadcaster, UnitEvent, UnitStatus, UnitStatusEvent};
use units::jig::JigDescription;
use units::test::TestDescription;
use units::scenario::ScenarioDescription;
use unitlibrary::UnitLibrary;

pub struct UnitLoader {
    broadcaster: UnitBroadcaster,
    receiver: Receiver<UnitEvent>,
    config: Arc<Mutex<Config>>,
    library: Arc<Mutex<UnitLibrary>>,
}

impl UnitLoader {
    pub fn new(broadcaster: &UnitBroadcaster, config: &Arc<Mutex<Config>>, library: &Arc<Mutex<UnitLibrary>>) -> Self {
        UnitLoader {
            broadcaster: broadcaster.clone(),
            receiver: broadcaster.subscribe(),
            config: config.clone(),
            library: library.clone(),
        }
    }

    fn handle_status(&self, event: &UnitStatusEvent) {
        match event.status() {
            &UnitStatus::Added(ref path) => self.load(event.name(), path),
            &UnitStatus::Updated(ref path) => {
                self.unload(event.name(), path);
                self.load(event.name(), path)
            }
            &UnitStatus::Removed(ref path) => self.unload(event.name(), path),
            _ => (),
        }
    }

    pub fn process_messages(&self) {
        while let Ok(msg) = self.receiver.recv() {
            match msg {
                UnitEvent::Shutdown => return,
                UnitEvent::Status(evt) => self.handle_status(&evt),
                UnitEvent::Category(_) => (),
            }
        }
    }

    pub fn load(&self, name: &UnitName, path: &PathBuf) {
        self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_load_started(name)));

        // For now, we only support testing Jig
        match name.kind() {
            &UnitKind::Jig => {

                // Ensure the jig is valid, has valid syntax, and can be loaded
                let jig_description = match JigDescription::from_path(path) {
                    Err(e) => {
                        self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_load_failed(name, format!("{}", e))));
                        return;
                    }
                    Ok(o) => o,
                };

                self.library.lock().unwrap().update_jig_description(jig_description);
            }

            &UnitKind::Test => {
                // Ensure the jig is valid, has valid syntax, and can be loaded
                let test_description = match TestDescription::from_path(path) {
                    Err(e) => {
                        self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_load_failed(name, format!("{}", e))));
                        return;
                    }
                    Ok(o) => o,
                };

                self.library.lock().unwrap().update_test_description(test_description);
            }

            &UnitKind::Scenario => {
                // Ensure the jig is valid, has valid syntax, and can be loaded
                let scenario_description = match ScenarioDescription::from_path(path) {
                    Err(e) => {
                        self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_load_failed(name, format!("{}", e))));
                        return;
                    }
                    Ok(o) => o,
                };

                self.library.lock().unwrap().update_scenario_description(scenario_description);
            }
        }
    }

    pub fn unload(&self, name: &UnitName, path: &PathBuf) {
        match name.kind() {
            &UnitKind::Jig => self.library.lock().unwrap().remove_jig(name),
            &UnitKind::Test => self.library.lock().unwrap().remove_test(name),
            &UnitKind::Scenario => self.library.lock().unwrap().remove_scenario(name),
        }
    }
}