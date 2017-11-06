use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time;

use unitmanager::ManagerControlMessage;
use unit::{UnitKind, UnitName};

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub enum UnitStatus {
    /// A new unit file has appeared on the disk
    Added(PathBuf),

    /// A unit file on the disk has changed, and the unit will be reloaded
    Updated(PathBuf),

    /// We started to load the unit file
    LoadStarted(PathBuf /* path to the unit file that's gong away */),

    /// The unit file failed to load for some reason
    LoadFailed(String /* reason */),

    /// The unit file reported that it was not compatible
    Incompatible(String /* reason */),

    /// The unit file has been loaded from disk, and may be selected.
    Loaded,

    /// The unit has been selected, and may be made active later on.
    /// For some unit types (e.g. Jig, Scenario), only one unit may be
    /// Selected at a time.
    Selected,

    /// The unit couldn't be selected for some reason.
    SelectFailed(String /* reason */),

    /// The unit has been deselected (but is still loaded, and may be selected later)
    Deselected(String /* reason */),

    /// The unit is currently in use
    Active,

    /// We tried to activate, but failed to do so.  This may happen with or without
    /// an "Active" message being sent.  I.e. if the unit is Selected and attempts
    /// to move into the Active state but fails, then ActivationFailed will be sent.
    /// If instead the unit is Active for a while but then fails at a later time,
    /// ActivationFailed will be sent.
    ActivationFailed(String /* reason */),

    /// The unit was active, then stopped being active due to finishing successfully
    DeactivatedSuccessfully(String /* reason */),

    /// The unit was active, then stopped being active due to finishing unsuccessfully
    DeactivatedUnsuccessfully(String /* reason */),

    /// The unit already successfully loaded, but is being removed
    UnloadStarted(PathBuf /* path to the unit file that's gong away */),

    /// The unit already successfully loaded, but is being updated
    UpdateStarted(PathBuf /* path to the unit file that's gong away */),

    /// The unit file was removed from the disk
    Removed(PathBuf),
}

impl fmt::Display for UnitStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &UnitStatus::Added(ref path) => write!(f, "Added file {}", path.to_string_lossy()),
            &UnitStatus::Updated(ref path) => write!(f, "Updated file {}", path.to_string_lossy()),
            &UnitStatus::LoadStarted(ref path) => write!(f, "Load started {}", path.to_string_lossy()),
            &UnitStatus::LoadFailed(ref x) => write!(f, "Load failed: {}", x),
            &UnitStatus::Incompatible(ref x) => write!(f, "Incompatible: {}", x),
            &UnitStatus::Loaded => write!(f, "Loaded"),
            &UnitStatus::Selected => write!(f, "Selected"),
            &UnitStatus::SelectFailed(ref reason) => write!(f, "Select failed: {}", reason),
            &UnitStatus::Deselected(ref reason) => write!(f, "Deselected: {}", reason),
            &UnitStatus::Active => write!(f, "Active"),
            &UnitStatus::ActivationFailed(ref reason) => write!(f, "Activation failed: {}", reason),
            &UnitStatus::DeactivatedSuccessfully(ref x) => {
                write!(f, "Deactivated successfully: {}", x)
            }
            &UnitStatus::DeactivatedUnsuccessfully(ref x) => {
                write!(f, "Deactivated unsuccessfilly: {}", x)
            }
            &UnitStatus::UnloadStarted(ref path) => write!(f, "Unloading {}", path.to_string_lossy()),
            &UnitStatus::UpdateStarted(ref path) => write!(f, "Updating {}", path.to_string_lossy()),
            &UnitStatus::Removed(ref path) => write!(f, "Removed file {}", path.to_string_lossy()),
        }
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct UnitStatusEvent {
    pub name: UnitName,
    pub status: UnitStatus,
}

impl UnitStatusEvent {
    pub fn name(&self) -> &UnitName {
        &self.name
    }
    pub fn status(&self) -> &UnitStatus {
        &self.status
    }
    pub fn kind(&self) -> &UnitKind {
        &self.name.kind()
    }
    pub fn new_added(path: &Path) -> Option<UnitStatusEvent> {
        let name = match UnitName::from_path(path) {
            Err(_) => return None,
            Ok(s) => s,
        };

        Some(UnitStatusEvent {
            name: name,
            status: UnitStatus::Added(path.to_owned()),
        })
    }
    pub fn new_updated(path: &Path) -> Option<UnitStatusEvent> {
        let name = match UnitName::from_path(path) {
            Err(_) => return None,
            Ok(s) => s,
        };

        Some(UnitStatusEvent {
            name: name,
            status: UnitStatus::Updated(path.to_owned()),
        })
    }
    pub fn new_removed(path: &Path) -> Option<UnitStatusEvent> {
        let name = match UnitName::from_path(path) {
            Err(_) => return None,
            Ok(s) => s,
        };

        Some(UnitStatusEvent {
            name: name,
            status: UnitStatus::Removed(path.to_owned()),
        })
    }

    pub fn new_selected(name: &UnitName) -> UnitStatusEvent {
        UnitStatusEvent {
            name: name.clone(),
            status: UnitStatus::Selected,
        }
    }

    pub fn new_select_failed(name: &UnitName, msg: String) -> UnitStatusEvent {
        UnitStatusEvent {
            name: name.clone(),
            status: UnitStatus::SelectFailed(msg),
        }
    }

    pub fn new_loaded(name: &UnitName) -> UnitStatusEvent {
        UnitStatusEvent {
            name: name.clone(),
            status: UnitStatus::Loaded,
        }
    }

    pub fn new_load_started(name: &UnitName, path: &PathBuf) -> UnitStatusEvent {
        UnitStatusEvent {
            name: name.clone(),
            status: UnitStatus::LoadStarted(path.clone()),
        }
    }

    pub fn new_update_started(name: &UnitName, path: &PathBuf) -> UnitStatusEvent {
        UnitStatusEvent {
            name: name.clone(),
            status: UnitStatus::UpdateStarted(path.clone()),
        }
    }

    pub fn new_load_failed(name: &UnitName, msg: String) -> UnitStatusEvent {
        UnitStatusEvent {
            name: name.clone(),
            status: UnitStatus::LoadFailed(msg),
        }
    }

    pub fn new_active(name: &UnitName) -> UnitStatusEvent {
        UnitStatusEvent {
            name: name.clone(),
            status: UnitStatus::Active,
        }
    }

    pub fn new_active_failed(name: &UnitName, msg: String) -> UnitStatusEvent {
        UnitStatusEvent {
            name: name.clone(),
            status: UnitStatus::ActivationFailed(msg),
        }
    }

    pub fn new_deactivate_success(name: &UnitName, msg: String) -> UnitStatusEvent {
        UnitStatusEvent {
            name: name.clone(),
            status: UnitStatus::DeactivatedSuccessfully(msg),
        }
    }

    pub fn new_deactivate_failure(name: &UnitName, msg: String) -> UnitStatusEvent {
        UnitStatusEvent {
            name: name.clone(),
            status: UnitStatus::DeactivatedUnsuccessfully(msg),
        }
    }

    pub fn new_unit_incompatible(name: &UnitName, msg: String) -> UnitStatusEvent {
        UnitStatusEvent {
            name: name.clone(),
            status: UnitStatus::Incompatible(msg),
        }
    }

    pub fn new_deselected(name: &UnitName, msg: String) -> UnitStatusEvent {
        UnitStatusEvent {
            name: name.clone(),
            status: UnitStatus::Deselected(msg),
        }
    }

    pub fn new_unload_started(name: &UnitName, path: &PathBuf) -> UnitStatusEvent {
        UnitStatusEvent {
            name: name.clone(),
            status: UnitStatus::UnloadStarted(path.clone()),
        }
    }
}

pub type UnitCategoryStatus = String;

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct UnitCategoryEvent {
    kind: UnitKind,
    status: UnitCategoryStatus,
}

impl UnitCategoryEvent {
    pub fn new(kind: UnitKind, status: &UnitCategoryStatus) -> Self {
        UnitCategoryEvent {
            kind: kind,
            status: status.clone(),
        }
    }
    pub fn kind(&self) -> &UnitKind {
        &self.kind
    }
    pub fn status(&self) -> &UnitCategoryStatus {
        &self.status
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub enum LogType {
    Error,
    Info,
}

impl LogType {
    pub fn as_str(&self) -> &str {
        match self {
            &LogType::Error => "error",
            &LogType::Info => "info",
        }
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct LogEntry {
    unit: UnitName,
    log_type: LogType,
    log_message: String,
    /// Number of seconds since the epoch
    pub unix_time: u64,

    /// Number of nanoseconds since the epoch
    pub unix_time_nsecs: u32,
}

impl LogEntry {
    pub fn new_error(id: UnitName, message: String) -> Self {
        let elapsed = Self::elapsed();
        LogEntry {
            unit: id,
            log_type: LogType::Error,
            log_message: message,
            unix_time: elapsed.as_secs(),
            unix_time_nsecs: elapsed.subsec_nanos(),
        }
    }

    pub fn new_info(id: UnitName, message: String) -> Self {
        let elapsed = Self::elapsed();
        LogEntry {
            unit: id,
            log_type: LogType::Info,
            log_message: message,
            unix_time: elapsed.as_secs(),
            unix_time_nsecs: elapsed.subsec_nanos(),
        }
    }

    pub fn secs(&self) -> u64 {
        self.unix_time
    }

    pub fn nsecs(&self) -> u32 {
        self.unix_time_nsecs
    }

    pub fn message(&self) -> &String {
        &self.log_message
    }

    pub fn kind(&self) -> &LogType {
        &self.log_type
    }

    pub fn id(&self) -> &UnitName {
        &self.unit
    }

    fn elapsed() -> time::Duration {
        let now = time::SystemTime::now();
        match now.duration_since(time::UNIX_EPOCH) {
            Ok(d) => d,
            Err(_) => time::Duration::new(0, 0),
        }
    }
}

impl fmt::Display for LogEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.log_type {
            LogType::Error => write!(f, "ERROR {}: {}", self.unit, self.log_message),
            LogType::Info => write!(f, "INFO {}: {}", self.unit, self.log_message),
        }
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub enum UnitEvent {
    /// A unit has updated its status.
    Status(UnitStatusEvent),

    /// A whole category of units has been updated.
    Category(UnitCategoryEvent),

    /// A generic log message.
    Log(LogEntry),

    /// The system has requested a rescan take place.
    RescanRequest,

    /// A rescan has started.
    RescanStart,

    /// The rescan has finished.
    RescanFinish,

    /// A unit made a request to a Manager, which will be passed to the main thread.
    ManagerRequest(ManagerControlMessage),

    /// The system is shutting down.
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct UnitBroadcaster {
    senders: Arc<Mutex<Vec<Sender<UnitEvent>>>>,
}

impl UnitBroadcaster {
    pub fn new() -> Self {
        UnitBroadcaster { senders: Arc::new(Mutex::new(vec![])) }
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

    pub fn broadcast(&self, event: &UnitEvent) {
        Self::broadcast_core(&self.senders, event)
    }

    pub fn subscribe(&self) -> Receiver<UnitEvent> {
        let (sender, receiver) = channel();
        self.senders.lock().unwrap().push(sender);
        receiver
    }

    pub fn log(&self, section: &str, message: String) {
        self.broadcast(&UnitEvent::Log(LogEntry::new_info(UnitName::internal(section), message)));
    }
}
