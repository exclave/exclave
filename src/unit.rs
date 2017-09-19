extern crate runny;

use std::fmt;
use std::path::Path;
use self::runny::RunnyError;

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

pub enum UnitIncompatibleReason {
    TestProgramReturnedNonzero(i32, String),
    TestProgramFailed(String),
    TestFileNotPresent(String),
}

impl fmt::Display for UnitIncompatibleReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &UnitIncompatibleReason::TestProgramFailed(ref program_name) => {
                write!(f, "Test program {} failed", program_name)
            }
            &UnitIncompatibleReason::TestProgramReturnedNonzero(val, ref program_name) => {
                write!(f, "Test program {} returned {}", program_name, val)
            }
            &UnitIncompatibleReason::TestFileNotPresent(ref file_name) => {
                write!(f, "Test file {} not present", file_name)
            }
        }
    }
}

impl From<RunnyError> for UnitIncompatibleReason {
    fn from(error: RunnyError) -> Self {
        match error {
            RunnyError::NoCommandSpecified => {
                UnitIncompatibleReason::TestProgramFailed("No command specified".to_owned())
            }
            RunnyError::RunnyIoError(ref e) => {
                UnitIncompatibleReason::TestProgramFailed(format!("Error running test program: {}",
                                                                 e))
            }
        }
    }
}

pub enum UnitSelectError {
    MissingDependency(UnitName /* This unit */, UnitName /* Wanted dependency */),
    UnitIncompatible(UnitName /* This unit */, UnitName /* Thing it is incompatible with */),
}

impl fmt::Display for UnitSelectError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &UnitSelectError::UnitIncompatible(ref name, ref other) => {
                write!(f, "Unit {} is incompatible with {}", name, other)
            }
            &UnitSelectError::MissingDependency(ref name, ref dep) => {
                write!(f, "Unit {} depends on {} which was not found", name, dep)
            }
        }
    }
}

pub enum UnitActivateError {
}

impl fmt::Display for UnitActivateError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Unable to activate unit")
    }
}

pub enum UnitDeactivateError {
}

impl fmt::Display for UnitDeactivateError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Unable to deactivate unit")
    }
}