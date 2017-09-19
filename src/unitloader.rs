use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use config::Config;
use unit::{UnitName, UnitKind};
use unitbroadcaster::{UnitBroadcaster, UnitEvent, UnitStatus, UnitStatusEvent};
use units::jig::JigDescription;

pub struct UnitLoader {
    broadcaster: UnitBroadcaster,
    receiver: Receiver<UnitEvent>,
    config: Arc<Mutex<Config>>,
}

impl UnitLoader {
    pub fn new(broadcaster: &UnitBroadcaster, config: &Arc<Mutex<Config>>) -> Self {
        UnitLoader {
            broadcaster: broadcaster.clone(),
            receiver: broadcaster.subscribe(),
            config: config.clone(),
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
        if name.kind() == &UnitKind::Jig {
            let jig_description = match JigDescription::from_path(path) {
                Err(e) => {
                    self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_load_failed(name, format!("{}", e))));
                    return;
                }
                Ok(o) => o,
            };

            if let Err(e) = jig_description.is_compatible(&*self.config.lock().unwrap()) {
                self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_unit_incompatible(name, format!("{}", e))));
                return;
            }
        }
    }

    pub fn unload(&self, name: &UnitName, path: &PathBuf) {
        if let Some(s) = UnitStatusEvent::new_unloading(path) {
            self.broadcaster.broadcast(&UnitEvent::Status(s));
        }
    }
}