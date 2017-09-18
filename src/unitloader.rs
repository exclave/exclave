use std::path::PathBuf;
use unitbroadcaster::{UnitBroadcaster, UnitEvent, UnitStatus, UnitStatusEvent};
use std::sync::mpsc::Receiver;

pub struct UnitLoader {
    broadcaster: UnitBroadcaster,
    receiver: Receiver<UnitEvent>,
}

impl UnitLoader {
    pub fn new(broadcaster: &UnitBroadcaster) -> Self {
        UnitLoader {
            broadcaster: broadcaster.clone(),
            receiver: broadcaster.subscribe(),
        }
    }

    fn handle_status(&self, event: &UnitStatusEvent) {
        match event.status() {
            &UnitStatus::Added(ref path) => self.load(path),
            &UnitStatus::Updated(ref path) => {
                self.load(path);
                self.unload(path)
            }
            &UnitStatus::Removed(ref path) => self.unload(path),
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

    pub fn load(&self, path: &PathBuf) {
        if let Some(s) = UnitStatusEvent::new_load_started(path) {
            self.broadcaster.broadcast(&UnitEvent::Status(s));
        }
    }

    pub fn unload(&self, path: &PathBuf) {
        if let Some(s) = UnitStatusEvent::new_unloading(path) {
            self.broadcaster.broadcast(&UnitEvent::Status(s));
        }
    }
}