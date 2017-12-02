extern crate runny;
extern crate systemd_parser;

use std::cell::RefCell;
use std::fs::File;
use std::io::{Read, Write, Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::Duration;

use config::Config;
use unit::{UnitActivateError, UnitDeactivateError, UnitDescriptionError, UnitIncompatibleReason, UnitSelectError, UnitDeselectError,
           UnitName};
use unitmanager::{ManagerControlMessage, ManagerControlMessageContents, ManagerStatusMessage,
                  UnitManager};

use self::systemd_parser::items::DirectiveEntry;
use self::runny::Runny;
use self::runny::running::Running;

#[derive(Clone, Copy)]
enum LoggerFormat {
    TSV,
    JSON,
}

/// A struct defining an in-memory representation of a .logger file
#[derive(Clone)]
pub struct LoggerDescription {
    /// The id of the unit (including the kind)
    id: UnitName,

    /// A short name
    name: String,

    /// A detailed description of this Logger, up to one paragraph.
    description: String,

    /// A Vec<String> of jig names that this test is compatible with.
    jigs: Vec<UnitName>,

    /// Path to the command to start the logger
    exec_start: String,

    /// The format expected by the logger
    format: LoggerFormat,

    /// The working directory to start from when running the logger
    working_directory: Option<PathBuf>,

    /// How long to wait for a terminate() call
    terminate_timeout: Duration,
}

impl LoggerDescription {
    pub fn from_path(path: &Path) -> Result<LoggerDescription, UnitDescriptionError> {
        let unit_name = UnitName::from_path(path)?;

        // Parse the file into a systemd unit_file object
        let mut contents = String::with_capacity(8192);
        File::open(path)?.read_to_string(&mut contents)?;
        let unit_file = systemd_parser::parse_string(&contents)?;

        if !unit_file.has_category("Logger") {
            return Err(UnitDescriptionError::MissingSection("Logger".to_owned()));
        }

        let mut logger_description = LoggerDescription {
            id: unit_name,
            name: "".to_owned(),
            description: "".to_owned(),
            jigs: vec![],
            format: LoggerFormat::TSV,
            exec_start: "".to_owned(),
            working_directory: None,
            terminate_timeout: Duration::from_secs(5),
        };

        for entry in unit_file.lookup_by_category("Logger") {
            match entry {
                &DirectiveEntry::Solo(ref directive) => match directive.key() {
                    "Name" => {
                        logger_description.name = directive.value().unwrap_or("").to_owned()
                    }
                    "Description" => {
                        logger_description.description =
                            directive.value().unwrap_or("").to_owned()
                    }
                    "Jigs" => {
                        logger_description.jigs = match directive.value() {
                            Some(s) => UnitName::from_list(s, "jig")?,
                            None => vec![],
                        }
                    }
                    "WorkingDirectory" => {
                        logger_description.working_directory =
                            Some(Path::new(directive.value().unwrap_or("")).to_owned())
                    }
                    "ExecStart" => {
                        logger_description.exec_start = match directive.value() {
                            Some(s) => s.to_owned(),
                            None => {
                                return Err(UnitDescriptionError::MissingValue(
                                    "Logger".to_owned(),
                                    "ExecStart".to_owned(),
                                ))
                            }
                        }
                    }
                    "Format" => {
                        logger_description.format = match directive.value() {
                            None => LoggerFormat::TSV,
                            Some(s) => match s.to_string().to_lowercase().as_ref() {
                                "tsv" => LoggerFormat::TSV,
                                "json" => LoggerFormat::JSON,
                                other => {
                                    return Err(UnitDescriptionError::InvalidValue(
                                        "Logger".to_owned(),
                                        "Format".to_owned(),
                                        other.to_owned(),
                                        vec!["tsv".to_owned(), "json".to_owned()],
                                    ))
                                }
                            },
                        }
                    }
                    &_ => (),
                },
                &_ => (),
            }
        }
        Ok(logger_description)
    }

    /// Returns true if this test is supported on the named jig.
    pub fn supports_jig(&self, name: &UnitName) -> bool {
        self.jigs.contains(name)
    }

    /// Determine if a unit is compatible with this system.
    pub fn is_compatible(
        &self,
        manager: &UnitManager,
        _: &Config,
    ) -> Result<(), UnitIncompatibleReason> {
        if self.jigs.len() == 0 {
            return Ok(());
        }
        for jig_name in &self.jigs {
            if manager.jig_is_loaded(&jig_name) {
                return Ok(());
            }
        }
        Err(UnitIncompatibleReason::IncompatibleJig)
    }

    pub fn id(&self) -> &UnitName {
        &self.id
    }

    pub fn load(
        &self,
        manager: &UnitManager,
        config: &Config,
    ) -> Result<Logger, UnitIncompatibleReason> {
        self.is_compatible(manager, config)?;

        Ok(Logger::new(self, manager, config))
    }
}

pub struct Logger {
    description: LoggerDescription,
    process: RefCell<Option<Running>>,
}

impl Logger {
    pub fn new(desc: &LoggerDescription, _: &UnitManager, _config: &Config) -> Logger {
        Logger {
            description: desc.clone(),
            process: RefCell::new(None),
        }
    }

    pub fn id(&self) -> &UnitName {
        &self.description.id
    }

    pub fn select(&self) -> Result<(), UnitSelectError> {
        Ok(())
    }

    pub fn deselect(&self) -> Result<(), UnitDeselectError> {
        Ok(())
    }

    pub fn activate(
        &self,
        manager: &UnitManager,
        config: &Config,
    ) -> Result<(), UnitActivateError> {
        let mut running = Runny::new(self.description.exec_start.as_str())
                    .directory(&Some(config.working_directory(&self.description.working_directory)))
                    .start()?;

        // Close stdout and stderr.
        running.take_output();
        running.take_error();

        let control_sender = manager.get_control_channel();
        let control_sender_id = self.id().clone();

        *self.process.borrow_mut() = Some(running);

        // Send some initial configuration to the client.
        control_sender.send(ManagerControlMessage::new(&control_sender_id, ManagerControlMessageContents::InitialGreeting)).ok();

        Ok(())
    }

    pub fn deactivate(&self) -> Result<(), UnitDeactivateError> {
        if let Some(process) = self.process.borrow_mut().take() {
            match process.terminate(Some(self.description.terminate_timeout)) {
                Ok(retval) => match retval {
                    0 => Ok(()),
                    i => Err(UnitDeactivateError::NonZeroReturn(i)),
                },
                Err(e) => Err(UnitDeactivateError::RunningError(e)),
            }
        }
        else {
            Ok(())
        }
    }

    /// Cause a MessageControlContents to be written out.
    pub fn output_message(&self, msg: ManagerStatusMessage) -> Result<(), Error> {
        match self.description.format {
            LoggerFormat::TSV => self.tsv_write(msg),
            LoggerFormat::JSON => self.json_write(msg),
        }
    }

    fn json_write(&self, _: ManagerStatusMessage) -> Result<(), Error> {
        unimplemented!();
    }

    fn cfti_escape(msg: &String) -> String {
        msg.replace("\\", "\\\\")
            .replace("\t", "\\t")
            .replace("\n", "\\n")
            .replace("\r", "\\r")
    }

    /// Write a ManagerStatusMessage to a TSV-formatted output.
    fn tsv_write(&self, msg: ManagerStatusMessage) -> Result<(), Error> {
        let mut process_opt = self.process.borrow_mut();

        if process_opt.is_none() {
            return Err(Error::new(ErrorKind::Other, "no process running"));
        }

        let process = process_opt.as_mut().unwrap();

        match msg {
            ManagerStatusMessage::Log(l) => writeln!(
                process,
                "{}\t{}\t{}\t{}\t{}\t{}",
                l.kind().as_str(),
                Self::cfti_escape(l.id().id()),
                Self::cfti_escape(&format!("{}", l.id().kind())),
                l.secs(),
                l.nsecs(),
                Self::cfti_escape(l.message())
            ),
            _ => Ok(()),
        }
    }
}
