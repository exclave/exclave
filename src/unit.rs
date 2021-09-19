extern crate dependy;
extern crate humantime;
extern crate regex;
extern crate runny;
extern crate systemd_parser;

use std::fmt;
use std::io;
use std::path::Path;

use self::dependy::DepError;
use self::humantime::DurationError;
use self::runny::running::RunningError;
use self::runny::RunnyError;
use self::systemd_parser::errors::ParserError;

#[derive(PartialEq, Eq, Hash, Debug, Clone, PartialOrd, Ord, Serialize)]
pub enum UnitKind {
    Interface,
    Jig,
    Logger,
    Scenario,
    Test,
    Trigger,

    /// Exclave-generated types
    Internal,
}

impl fmt::Display for UnitKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            UnitKind::Interface => write!(f, "interface"),
            UnitKind::Internal => write!(f, "internal"),
            UnitKind::Jig => write!(f, "jig"),
            UnitKind::Logger => write!(f, "logger"),
            UnitKind::Scenario => write!(f, "scenario"),
            UnitKind::Test => write!(f, "test"),
            UnitKind::Trigger => write!(f, "trigger"),
        }
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone, PartialOrd, Ord, Serialize)]
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
            UnitNameError::NoFileExtension => write!(f, "no file extension"),
            UnitNameError::UnrecognizedUnitType(ref t) => {
                write!(f, "unrecognized unit type \".{}\"", t)
            }
        }
    }
}

impl UnitName {
    pub fn kind(&self) -> &UnitKind {
        &self.kind
    }

    pub fn id(&self) -> &String {
        &self.id
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
            "interface" => UnitKind::Interface,
            "jig" => UnitKind::Jig,
            "logger" => UnitKind::Logger,
            "scenario" => UnitKind::Scenario,
            "test" => UnitKind::Test,
            "trigger" => UnitKind::Trigger,
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
            Self::from_path(Path::new(&new_path))
        } else {
            Self::from_path(path)
        };
        result
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

    pub fn internal(s: &str) -> Self {
        UnitName {
            id: s.to_owned(),
            kind: UnitKind::Internal,
        }
    }
}

impl fmt::Display for UnitName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}", self.id, self.kind)
    }
}

#[derive(Debug)]
pub enum UnitIncompatibleReason {
    TestProgramReturnedNonzero(i32, String),
    TestProgramFailed(String),
    TestFileNotPresent(String),
    IncompatibleJig,
    DependencyError(DepError<UnitName>),
}

impl fmt::Display for UnitIncompatibleReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            UnitIncompatibleReason::TestProgramFailed(ref program_name) => {
                write!(f, "Test program {} failed", program_name)
            }
            UnitIncompatibleReason::TestProgramReturnedNonzero(val, ref program_name) => {
                write!(f, "Test program {} returned {}", program_name, val)
            }
            UnitIncompatibleReason::TestFileNotPresent(ref file_name) => {
                write!(f, "Test file {} not present", file_name)
            }
            UnitIncompatibleReason::IncompatibleJig => write!(f, "Jig not compatible"),
            UnitIncompatibleReason::DependencyError(ref dep_error) => match dep_error {
                DepError::RequirementsNotFound(ref req) => {
                    write!(f, "Requirement '{}' not found", req)
                }
                DepError::RequirementNotFound(ref req1, ref req2) => {
                    write!(f, "Requirement {} not found for {}", req1, req2)
                }
                DepError::SuggestionsNotFound(ref req) => {
                    write!(f, "Suggestion '{}' not found", req)
                }
                DepError::SuggestionNotFound(ref req1, ref req2) => {
                    write!(f, "Suggestion {} not found for {}", req1, req2)
                }
                DepError::DependencyNotFound(ref name) => {
                    write!(f, "Dependency '{}' not found", name)
                }
                DepError::CircularDependency(ref req1, ref req2) => {
                    write!(f, "{} and {} have a circular dependency", req1, req2)
                }
            },
        }
    }
}

impl From<RunnyError> for UnitIncompatibleReason {
    fn from(error: RunnyError) -> Self {
        match error {
            RunnyError::NoCommandSpecified => {
                UnitIncompatibleReason::TestProgramFailed("No command specified".to_owned())
            }
            RunnyError::RunnyIoError(ref e) => UnitIncompatibleReason::TestProgramFailed(format!(
                "Error running test program: {}",
                e
            )),
            #[cfg(unix)]
            RunnyError::NixError(ref e) => {
                UnitIncompatibleReason::TestProgramFailed(format!("Unix error {}", e))
            }
        }
    }
}

impl From<DepError<UnitName>> for UnitIncompatibleReason {
    fn from(error: DepError<UnitName>) -> Self {
        UnitIncompatibleReason::DependencyError(error)
    }
}

pub enum UnitSelectError {
    /// We tried to select a unit based on its ID, but it wasn't found.
    UnitNotFound,

    /// This unit is for a jig that is not loaded.
    NoCompatibleJig,
}

impl fmt::Display for UnitSelectError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            UnitSelectError::UnitNotFound => write!(f, "couldn't find unit by ID"),
            UnitSelectError::NoCompatibleJig => write!(f, "couldn't find any compatible jig"),
        }
    }
}

pub enum UnitDeselectError {
    /// We tried to select a unit based on its ID, but it wasn't found.
    UnitNotFound,
}

impl fmt::Display for UnitDeselectError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "couldn't find unit by ID")
    }
}

pub enum UnitActivateError {
    /// We tried to activate a unit based on its ID, but it wasn't found.
    UnitNotFound,

    /// Tried to activate a unit that wasn't selected.
    UnitNotSelected,

    /// We tried to Exec, but an error occurred.
    ExecFailed(RunnyError),
}

impl From<RunnyError> for UnitActivateError {
    fn from(kind: RunnyError) -> Self {
        UnitActivateError::ExecFailed(kind)
    }
}

impl fmt::Display for UnitActivateError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            UnitActivateError::ExecFailed(ref re) => match re {
                RunnyError::RunnyIoError(ref e) => {
                    write!(f, "Unable to activate unit: {}", e)
                }
                RunnyError::NoCommandSpecified => {
                    write!(f, "Unable to activate unit: No command specified")
                }
                #[cfg(unix)]
                RunnyError::NixError(ref e) => {
                    write!(f, "Unable to activate unit: Nix library error: {:?}", e)
                }
            },
            UnitActivateError::UnitNotFound => write!(f, "Couldn't find unit by id"),
            UnitActivateError::UnitNotSelected => write!(f, "Tried to activate a deselected unit"),
        }
    }
}

pub enum UnitDeactivateError {
    /// We tried to deactivate a unit based on its ID, but it wasn't found.
    UnitNotFound,

    /// The unit returned something other than 0.
    NonZeroReturn(i32),

    /// Runny reported an error of some sort.
    RunningError(RunningError),
}

impl From<RunningError> for UnitDeactivateError {
    fn from(error: RunningError) -> Self {
        UnitDeactivateError::RunningError(error)
    }
}

impl fmt::Display for UnitDeactivateError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            UnitDeactivateError::NonZeroReturn(i) => write!(f, "Nonzero return: {}", i),
            UnitDeactivateError::RunningError(ref e) => write!(f, "Running error: {:?}", e),
            UnitDeactivateError::UnitNotFound => write!(f, "Couldn't find unit by id"),
        }
    }
}

#[derive(Debug)]
pub enum UnitDescriptionError {
    InvalidUnitName(UnitNameError),
    MissingSection(String /* section name */),
    MissingValue(String /* section name */, String /* key name */),
    FileOpenError(io::Error),
    ParseError(ParserError),
    RegexError(self::regex::Error),
    HumantimeError(DurationError),
    ParseIntError(std::num::ParseIntError),
    InvalidValue(
        String,      // Section name
        String,      // Key name
        String,      // Specified value
        Vec<String>, /* Allowed values */
    ),
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

impl From<DurationError> for UnitDescriptionError {
    fn from(error: DurationError) -> Self {
        UnitDescriptionError::HumantimeError(error)
    }
}

impl From<std::num::ParseIntError> for UnitDescriptionError {
    fn from(error: std::num::ParseIntError) -> Self {
        UnitDescriptionError::ParseIntError(error)
    }
}

impl fmt::Display for UnitDescriptionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            UnitDescriptionError::InvalidUnitName(ref reason) => {
                write!(f, "invalid unit name '{}'", reason)
            }
            UnitDescriptionError::MissingSection(ref sec) => {
                write!(f, "missing [{}] section", sec)
            }
            UnitDescriptionError::FileOpenError(ref e) => {
                write!(f, "unable to open file -- {}", e)
            }
            UnitDescriptionError::ParseError(ref e) => {
                write!(f, "syntax error: {}", e)
            }
            UnitDescriptionError::HumantimeError(ref e) => {
                write!(f, "time parse error: {}", e)
            }
            UnitDescriptionError::RegexError(ref e) => write!(f, "unable to parse regex: {}", e),
            UnitDescriptionError::MissingValue(ref sec, ref key) => {
                write!(f, "key '{}' in section '{}' requires a value", key, sec)
            }
            UnitDescriptionError::ParseIntError(ref e) => {
                write!(f, "int parse error: {}", e)
            }
            UnitDescriptionError::InvalidValue(ref sec, ref key, ref val, ref allowed) => write!(
                f,
                "key {} in section {} has invalid value: {}, must be one of: {}",
                key,
                sec,
                val,
                allowed.join(",")
            ),
        }
    }
}
