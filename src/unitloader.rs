use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::cell::RefCell;

use config::Config;
use unit::{UnitName, UnitKind};
use unitbroadcaster::{UnitBroadcaster, UnitEvent, UnitStatus, UnitStatusEvent, UnitCategoryEvent};
use units::jig::{JigDescription, Jig};
use units::test::{TestDescription, Test};

pub struct UnitLoader {
    broadcaster: UnitBroadcaster,
    receiver: Receiver<UnitEvent>,
    config: Arc<Mutex<Config>>,
    jigs: RefCell<HashMap<UnitName, Arc<Mutex<Jig>>>>,
}

impl UnitLoader {
    pub fn new(broadcaster: &UnitBroadcaster, config: &Arc<Mutex<Config>>) -> Self {
        UnitLoader {
            broadcaster: broadcaster.clone(),
            receiver: broadcaster.subscribe(),
            config: config.clone(),
            jigs: RefCell::new(HashMap::new()),
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
                self.jigs.borrow_mut().insert(name.clone(), Arc::new(Mutex::new(new_jig)));

                // Notify everyone this unit has been selected.
                self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_selected(name)));

                self.broadcaster.broadcast(&UnitEvent::Category(UnitCategoryEvent::new(UnitKind::Jig, &format!("Number of units loaded: {}", self.jigs.borrow().len()))));
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
            }
            _ => {}
        }
    }

    pub fn unload(&self, name: &UnitName, path: &PathBuf) {
        if let Some(s) = UnitStatusEvent::new_unloading(path) {
            self.broadcaster.broadcast(&UnitEvent::Status(s));
        }
    }
}