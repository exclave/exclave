extern crate notify;

use std::path::{Path, PathBuf};
use std::io;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::sync::{Mutex, Arc};
use std::fmt;
use std::time::Duration;
use std::thread;

use self::notify::{RecommendedWatcher, Watcher, RecursiveMode};

#[derive(PartialEq, Eq, Hash, Debug, Clone, PartialOrd, Ord)]
pub enum UnitKind {
    Jig,
    Scenario,
    Test,
}

impl fmt::Display for UnitKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &UnitKind::Jig => write!(f, "jig"),
            &UnitKind::Scenario => write!(f, "scenario"),
            &UnitKind::Test => write!(f, "test"),
        }
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone, PartialOrd, Ord)]
pub struct UnitName {
    id: String,
    kind: UnitKind,
}

impl UnitName {
    pub fn kind(&self) -> &UnitKind {
        &self.kind
    }
    pub fn id(&self) -> &String {
        &self.id
    }
}

impl fmt::Display for UnitName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}", self.id, self.kind)
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub enum UnitStatus {
    /// A new unit file has appeared on the disk
    Added,

    /// A unit file on the disk has changed, and the unit will be reloaded
    Updated,

    /// The unit file failed to load for some reason
    LoadStarted(String /* reason */),

    /// The unit file failed to load for some reason
    LoadFailed(String /* reason */),

    /// The unit file reported that it was not compatible
    UnitIncompatible(String /* reason */),

    /// The unit has been selected, and may be made active later on.
    UnitSelected,

    /// The unit has been deselected (but is still loaded, and may be selected later)
    UnitDeselected,

    /// The unit is currently in use
    UnitActive,

    /// The unit was active, then stopped being active due to finishing successfully
    UnitDeactivatedSuccessfully(String /* reason */),

    /// The unit was active, then stopped being active due to finishing unsuccessfully
    UnitDeactivatedUnsuccessfully(String /* reason */),

    /// The unit file was removed from the disk
    Deleted,
}

impl fmt::Display for UnitStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &UnitStatus::Added => write!(f, "Added"),
            &UnitStatus::Updated => write!(f, "Updated"),
            &UnitStatus::LoadStarted(ref x) => write!(f, "Load started: {}", x),
            &UnitStatus::LoadFailed(ref x) => write!(f, "Load failed: {}", x),
            &UnitStatus::UnitIncompatible(ref x) => write!(f, "Incompatible: {}", x),
            &UnitStatus::UnitSelected => write!(f, "Selected"),
            &UnitStatus::UnitDeselected => write!(f, "Deselected"),
            &UnitStatus::UnitActive => write!(f, "Active"),
            &UnitStatus::UnitDeactivatedSuccessfully(ref x) => {
                write!(f, "Deactivated successfully: {}", x)
            }
            &UnitStatus::UnitDeactivatedUnsuccessfully(ref x) => {
                write!(f, "Deactivated unsuccessfilly: {}", x)
            }
            &UnitStatus::Deleted => write!(f, "Deleted"),
        }
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct UnitStatusEvent {
    name: UnitName,
    status: UnitStatus,
}

impl UnitStatusEvent {
    pub fn name(&self) -> &UnitName {
        &self.name
    }
    pub fn status(&self) -> &UnitStatus {
        &self.status
    }
    pub fn kind(&self) -> &UnitKind {
        &self.name.kind
    }
}

pub type UnitCategoryStatus = String;

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct UnitCategoryEvent {
    pub kind: UnitKind,
    pub status: UnitCategoryStatus,
}
impl UnitCategoryEvent {
    pub fn kind(&self) -> &UnitKind {
        &self.kind
    }
    pub fn status(&self) -> &UnitCategoryStatus {
        &self.status
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub enum UnitEvent {
    Status(UnitStatusEvent),
    Category(UnitCategoryEvent),
}

pub struct UnitLoader {
    paths: Vec<PathBuf>,
    senders: Arc<Mutex<Vec<Sender<UnitEvent>>>>,
    watcher: RecommendedWatcher,
}

impl UnitLoader {
    pub fn new() -> UnitLoader {
        let senders = Arc::new(Mutex::new(vec![]));
        let (watcher_tx, watcher_rx) = channel();

        // Automatically select the best implementation for your platform.
        // You can also access each implementation directly e.g. INotifyWatcher.
        let watcher: RecommendedWatcher = Watcher::new(watcher_tx, Duration::from_secs(2))
            .expect("Unable to create file watcher");

        // This is a simple loop, but you may want to use more complex logic here,
        // for example to handle I/O.
        let notify_senders = senders.clone();
        thread::spawn(move || {
            loop {
                match watcher_rx.recv() {
                    Ok(event) => {
                        // Convert the DebouncedEvent into a UnitEvent
                        let status_event = match event {
                            notify::DebouncedEvent::Create(path) => {
                                UnitEvent::Status(UnitStatusEvent {
                                    name: Self::file_to_unit_name(&path).unwrap(),
                                    status: UnitStatus::Added,
                                })
                            }
                            notify::DebouncedEvent::Write(path) => {
                                UnitEvent::Status(UnitStatusEvent {
                                    name: Self::file_to_unit_name(&path).unwrap(),
                                    status: UnitStatus::Updated,
                                })
                            }
                            notify::DebouncedEvent::Remove(path) => {
                                UnitEvent::Status(UnitStatusEvent {
                                    name: Self::file_to_unit_name(&path).unwrap(),
                                    status: UnitStatus::Deleted,
                                })
                            }
                            _ => continue,
                        };

                        // Send a copy of the message to each of the listeners.
                        Self::broadcast_core(&notify_senders, &status_event);
                    }
                    Err(e) => println!("watch error: {:?}", e),
                }
            }
        });

        UnitLoader {
            paths: vec![],
            senders: senders,
            watcher: watcher,
        }
    }

    fn broadcast_core(senders: &Arc<Mutex<Vec<Sender<UnitEvent>>>>, event: &UnitEvent) {
        let mut to_remove = None;
        // Send a copy of the message to each of the listeners.
        let mut notify_senders_ref = senders.lock().unwrap();
        {
            for (idx, sender) in notify_senders_ref.iter().enumerate() {
                if let Err(e) = sender.send(event.clone()) {
                    eprintln!("Sender {} stopped responding: {:?}, removing it.", idx, e);
                    to_remove = Some(idx);
                }
            }
        }

        // If a sender threw an error, simply remove it from the list of elements to update
        if let Some(idx) = to_remove {
            notify_senders_ref.remove(idx);
        }
    }

    pub fn broadcast(&mut self, event: &UnitEvent) {
        Self::broadcast_core(&self.senders, event)
    }

    pub fn subscribe(&mut self) -> Receiver<UnitEvent> {
        let (sender, receiver) = channel();
        self.senders.lock().unwrap().push(sender);
        receiver
    }

    fn add_unit(&self, unit_name: UnitName) {
        for sender in self.senders.lock().unwrap().iter() {
            sender.send(UnitEvent::Status(UnitStatusEvent {
                    name: unit_name.clone(),
                    status: UnitStatus::Added,
                }))
                .expect("Failed to send notification to adding a unit.  Aborting.");
        }
    }

    fn file_to_unit_name(path: &Path) -> Option<UnitName> {

        // Get the extension.  An empty extension is 'valid'
        // although it will get rejected below.
        let extension = match path.extension() {
            None => "".to_owned(),
            Some(s) => s.to_str().unwrap_or("").to_owned(),
        };

        // Get the unit ID.  An empty unit ID is considered invalid.
        let unit_id = match path.file_stem() {
            None => return None,
            Some(s) => s.to_str().unwrap_or("").to_owned(),
        };

        // Perform the extension-to-unit-kind mapping.  Reject invalid
        // or unrecognized unit kinds.
        let unit_kind = match extension.as_str() {
            "jig" => UnitKind::Jig,
            "scenario" => UnitKind::Scenario,
            "test" => UnitKind::Test,
            _ => return None,
        };

        Some(UnitName {
            id: unit_id,
            kind: unit_kind,
        })
    }

    pub fn add_path(&mut self, config_dir: &str) -> Result<(), io::Error> {
        let dir = Path::new(config_dir);
        for entry in dir.read_dir()? {
            let unit_name = match Self::file_to_unit_name(&entry?.path()) {
                None => continue,
                Some(s) => s,
            };

            self.add_unit(unit_name);
        }

        self.watch(&dir).expect("Unable to watch directory");
        self.paths.push(dir.to_owned());
        Ok(())
    }

    fn watch(&mut self, path: &Path) -> notify::Result<()> {

        // Add a path to be watched. All files and directories at that path and
        // below will be monitored for changes.
        try!(self.watcher.watch(path, RecursiveMode::Recursive));

        Ok(())
    }
}