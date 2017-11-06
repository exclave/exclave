use std::path::PathBuf;

use unit::UnitName;
use unitbroadcaster::{UnitBroadcaster, UnitEvent, UnitStatus, UnitStatusEvent};

pub struct UnitLoader {
    broadcaster: UnitBroadcaster,
}

impl UnitLoader {
    pub fn new(broadcaster: &UnitBroadcaster)
               -> Self {
        UnitLoader {
            broadcaster: broadcaster.clone(),
        }
    }

    pub fn process_message(&self, msg: &UnitEvent) {
        match msg {
            &UnitEvent::Shutdown => return,
            &UnitEvent::Status(ref evt) => self.handle_status(evt),
            &UnitEvent::RescanRequest => (),
            &UnitEvent::RescanStart => (),
            &UnitEvent::RescanFinish => (),
            &UnitEvent::Category(_) => (),
            &UnitEvent::Log(_) => (),
            &UnitEvent::ManagerRequest(_) => (),
            &UnitEvent::ChildProgramExited(_, _) => (),
            &UnitEvent::RequestProgramExit(_) => (),
        }
    }

    fn handle_status(&self, event: &UnitStatusEvent) {
        match event.status() {
            &UnitStatus::Added(ref path) => self.load(event.name(), path),
            &UnitStatus::Updated(ref path) => self.update(event.name(), path),
            &UnitStatus::Removed(ref path) => self.unload(event.name(), path),
            _ => (),
        }
    }

    pub fn load(&self, name: &UnitName, path: &PathBuf) {
        self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_load_started(name, path)));
    }

    pub fn update(&self, name: &UnitName, path: &PathBuf) {
        self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_update_started(name, path)));
    }

    pub fn unload(&self, name: &UnitName, path: &PathBuf) {
        self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent::new_unload_started(name, path)));
    }
}