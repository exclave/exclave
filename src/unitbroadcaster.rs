use std::path::Path;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::sync::{Mutex, Arc};
use std::fmt;

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

    pub fn from_path(path: &Path) -> Option<Self> {

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
}