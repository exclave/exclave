use std::path::{Path, PathBuf};
use std::io;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::ffi::OsStr;
use std::fmt;

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub enum UnitKind {
    Jig,
    Scenario,
    Test,
}

impl fmt::Display for UnitKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Jig => write!(f, "jig"),
            Scenario => write!(f, "scenario"),
            Test => write!(f, "test"),
        }
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
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
            Added => write!(f, "Added"),
            Updated => write!(f, "Updated"),
            &UnitStatus::LoadStarted(ref x) => write!(f, "Load started: {}", x),
            &UnitStatus::LoadFailed(ref x) => write!(f, "Load failed: {}", x),
            &UnitStatus::UnitIncompatible(ref x) => write!(f, "Incompatible: {}", x),
            UnitSelected => write!(f, "Selected"),
            UnitDeselected => write!(f, "Deselected"),
            UnitActive => write!(f, "Active"),
            &UnitStatus::UnitDeactivatedSuccessfully(ref x) => write!(f, "Deactivated successfully: {}", x),
            &UnitStatus::UnitDeactivatedUnsuccessfully(ref x) => write!(f, "Deactivated unsuccessfilly: {}", x),
            Deleted => write!(f, "Deleted"),
        }
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct UnitStatusEvent {
    pub name: UnitName,
    pub status: UnitStatus,
}

pub struct UnitLoader {
    paths: Vec<PathBuf>,
    senders: Vec<Sender<UnitStatusEvent>>,
}

impl UnitLoader {
    pub fn new() -> UnitLoader {
        UnitLoader {
            paths: vec![],
            senders: vec![],
        }
    }

    pub fn subscribe(&mut self) -> Receiver<UnitStatusEvent> {
        let (sender, receiver) = channel();
        self.senders.push(sender);
        receiver
    }

    pub fn add_file(&mut self, unit_name: UnitName) {
        for sender in &self.senders {
            sender.send(UnitStatusEvent {
                name: unit_name.clone(),
                status: UnitStatus::Added,
            });
        }
    }

    pub fn add_path(&mut self, config_dir: &str) -> Result<(), io::Error> {
        let dir = Path::new(config_dir);
        for entry in dir.read_dir()? {
            let entry = entry?;

            // Only operate on files (and symlinks)
            {
                let file_type = entry.file_type()?;
                if !file_type.is_file() && !file_type.is_symlink() {
                    continue;
                }
            }

            // Pull out the extension.  An empty extension is 'valid'
            // although it will get rejected below.
            let extension = match entry.path().extension() {
                None => "".to_owned(),
                Some(s) => s.to_str().unwrap_or("").to_owned(),
            };

            // Pull out the unit ID.  An empty unit ID is considered invalid.
            let unit_id = match entry.path().file_stem() {
                None => continue,
                Some(s) => s.to_str().unwrap_or("").to_owned(),
            };

            // Perform the extension-to-unit-kind mapping.  Reject invalid
            // or unrecognized unit kinds.
            let unit_kind = match extension.as_str() {
                "jig" => UnitKind::Jig,
                "scenario" => UnitKind::Scenario,
                "test" => UnitKind::Test,
                _ => continue,
            };

            let unit_name = UnitName {
                id: unit_id,
                kind: unit_kind,
            };
            self.add_file(unit_name);
        }
        self.paths.push(dir.to_owned());
        Ok(())
    }
}