extern crate dependy;
extern crate regex;
extern crate runny;
extern crate systemd_parser;

use std::fmt;
use std::path::Path;
use std::io;

use self::dependy::DepError;
use self::runny::RunnyError;
use self::systemd_parser::errors::ParserError;

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

#[derive(Debug)]
pub enum UnitNameError {
    NoFileExtension,
    UnrecognizedUnitType(String),
}

impl fmt::Display for UnitNameError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &UnitNameError::NoFileExtension => write!(f, "no file extension"),
            &UnitNameError::UnrecognizedUnitType(ref t) => {
                write!(f, "unrecognized unit type \".{}\"", t)
            }
        }
    }
}

impl UnitName {
    pub fn kind(&self) -> &UnitKind {
        &self.kind
    }

    pub fn to_string(&self) -> String {
        format!("{}", self)
    }

    pub fn from_path(path: &Path) -> Result<Self, UnitNameError> {
        // Get the extension.  An empty extension is 'valid'
        // although it will get rejected below.
        let extension = match path.extension() {
            None => "".to_owned(),
            Some(s) => s.to_str().unwrap_or("").to_owned(),
        };

        // Get the unit ID.  An empty unit ID is considered invalid.
        let unit_id = match path.file_stem() {
            None => return Err(UnitNameError::NoFileExtension),
            Some(s) => s.to_str().unwrap_or("").to_owned(),
        };

        // Perform the extension-to-unit-kind mapping.  Reject invalid
        // or unrecognized unit kinds.
        let unit_kind = match extension.as_str() {
            "jig" => UnitKind::Jig,
            "scenario" => UnitKind::Scenario,
            "test" => UnitKind::Test,
            _ => return Err(UnitNameError::UnrecognizedUnitType(extension)),
        };

        Ok(UnitName {
            id: unit_id,
            kind: unit_kind,
        })
    }

    /// Generate a UnitName from the specified name.
    /// If no extension is present, use default_type as the extension.
    pub fn from_str(name: &str, default_type: &str) -> Result<Self, UnitNameError> {
        let path = Path::new(name);
        let result = if path.extension().is_none() {
            let new_path = format!("{}.{}", path.to_string_lossy(), default_type);
            Self::from_path(&Path::new(&new_path))
        } else {
            Self::from_path(&path)
        };
        return result;
    }

    pub fn from_list(s: &str, default_type: &str) -> Result<Vec<Self>, UnitNameError> {
        let in_list_list: Vec<&str> = s.split(|c| c == ',').collect();
        let mut out_list = vec![];
        for in_list in in_list_list {
            for item in in_list.split_whitespace() {
                out_list.push(UnitName::from_str(item, default_type)?);
            }
        }
        Ok(out_list)
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
    IncompatibleJig,
    DependencyError(DepError),
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
            &UnitIncompatibleReason::IncompatibleJig => write!(f, "Jig not compatible"),
            &UnitIncompatibleReason::DependencyError(ref dep_error) => {
                match dep_error {
                    &DepError::RequirementsNotFound(ref req) => {
                        write!(f, "Requirement '{}' not found", req)
                    }
                    &DepError::RequirementNotFound(ref req1, ref req2) => {
                        write!(f, "Requirement {} not found for {}", req1, req2)
                    }
                    &DepError::SuggestionsNotFound(ref req) => {
                        write!(f, "Suggestion '{}' not found", req)
                    }
                    &DepError::SuggestionNotFound(ref req1, ref req2) => {
                        write!(f, "Suggestion {} not found for {}", req1, req2)
                    }
                    &DepError::DependencyNotFound(ref name) => {
                        write!(f, "Dependency '{}' not found", name)
                    }
                    &DepError::CircularDependency(ref req1, ref req2) => {
                        write!(f, "{} and {} have a circular dependency", req1, req2)
                    }
                }
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
            #[cfg(unix)]
            RunnyError::NixError(ref e) => {
                UnitIncompatibleReason::TestProgramFailed(format!("Unix error {}", e))
            }
        }
    }
}

impl From<DepError> for UnitIncompatibleReason {
    fn from(error: DepError) -> Self {
        UnitIncompatibleReason::DependencyError(error)
    }
}

pub enum UnitActivateError {}

impl fmt::Display for UnitActivateError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Unable to activate unit")
    }
}

pub enum UnitDeactivateError {}

impl fmt::Display for UnitDeactivateError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Unable to deactivate unit")
    }
}

pub enum UnitDescriptionError {
    InvalidUnitName(UnitNameError),
    MissingSection(String /* section name */),
    FileOpenError(io::Error),
    ParseError(ParserError),
    RegexError(self::regex::Error),
    InvalidValue(String, // Section name
                 String, // Key name
                 String, // Specified value
                 Vec<String> /* Allowed values */),
}

impl From<UnitNameError> for UnitDescriptionError {
    fn from(kind: UnitNameError) -> Self {
        UnitDescriptionError::InvalidUnitName(kind)
    }
}

impl From<io::Error> for UnitDescriptionError {
    fn from(error: io::Error) -> Self {
        UnitDescriptionError::FileOpenError(error)
    }
}

impl From<self::systemd_parser::errors::ParserError> for UnitDescriptionError {
    fn from(error: self::systemd_parser::errors::ParserError) -> Self {
        UnitDescriptionError::ParseError(error)
    }
}

impl From<self::regex::Error> for UnitDescriptionError {
    fn from(error: self::regex::Error) -> Self {
        UnitDescriptionError::RegexError(error)
    }
}

impl fmt::Display for UnitDescriptionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use std::error::Error;
        match self {
            &UnitDescriptionError::InvalidUnitName(ref reason) => {
                write!(f, "Invalid unit name: {}", reason)
            }
            &UnitDescriptionError::MissingSection(ref sec) => {
                write!(f, "Missing [{}] section", sec)
            }
            &UnitDescriptionError::FileOpenError(ref e) => {
                write!(f, "Unable to open file: {}", e.description())
            }
            &UnitDescriptionError::ParseError(ref e) => {
                write!(f, "Syntax error: {}", e.description())
            }
            &UnitDescriptionError::RegexError(ref e) => write!(f, "Unable to parse regex: {}", e),
            &UnitDescriptionError::InvalidValue(ref sec, ref key, ref val, ref allowed) => {
                write!(f,
                       "Key {} in section {} has invalid value: {}.  Value must be one of: {}",
                       key,
                       sec,
                       val,
                       allowed.join(","))
            }
        }
    }
}