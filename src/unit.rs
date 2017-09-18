use std::fmt;
use std::path::Path;

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
