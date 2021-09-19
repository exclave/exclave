extern crate runny;
extern crate serde_json;
extern crate systemd_parser;

use std::cell::RefCell;
use std::fs::File;
use std::io::{BufRead, BufReader, Error, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use config::Config;
use unit::{
    UnitActivateError, UnitDeactivateError, UnitDescriptionError, UnitDeselectError,
    UnitIncompatibleReason, UnitName, UnitSelectError,
};
use unitbroadcaster::LogEntry;
use unitmanager::{
    ManagerControlMessage, ManagerControlMessageContents, ManagerStatusMessage, UnitManager,
};

use self::runny::running::{Running, RunningOutput};
use self::runny::Runny;
use self::systemd_parser::items::DirectiveEntry;

#[derive(Clone, Copy)]
enum LoggerFormat {
    Tsv,
    Json,
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

    /// The path to the unit file
    unit_directory: PathBuf,

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
            format: LoggerFormat::Tsv,
            exec_start: "".to_owned(),
            working_directory: None,
            unit_directory: path.parent().unwrap().to_owned(),
            terminate_timeout: Duration::from_secs(5),
        };

        for entry in unit_file.lookup_by_category("Logger") {
            if let DirectiveEntry::Solo(ref directive) = entry {
                match directive.key() {
                    "Name" => logger_description.name = directive.value().unwrap_or("").to_owned(),
                    "Description" => {
                        logger_description.description = directive.value().unwrap_or("").to_owned()
                    }
                    "Jigs" => {
                        logger_description.jigs = match directive.value() {
                            Some(s) => UnitName::from_list(s, "jig")?,
                            None => vec![],
                        }
                    }
                    "WorkingDirectory" => {
                        if let Some(wd) = directive.value() {
                            logger_description.working_directory = Some(PathBuf::from(wd));
                        }
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
                            None => LoggerFormat::Tsv,
                            Some(s) => match s.to_string().to_lowercase().as_ref() {
                                "tsv" => LoggerFormat::Tsv,
                                "json" => LoggerFormat::Json,
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
                }
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
        if self.jigs.is_empty() {
            return Ok(());
        }
        for jig_name in &self.jigs {
            if manager.jig_is_loaded(jig_name) {
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

    fn text_read(id: UnitName, control: Sender<ManagerControlMessage>, output: RunningOutput) {
        for line in BufReader::new(output).lines() {
            let line = line.expect("Unable to get next line");
            // If the send fails, that means the other end has closed the pipe.
            if control
                .send(ManagerControlMessage::new(
                    &id,
                    ManagerControlMessageContents::LogError(line),
                ))
                .is_err()
            {
                break;
            }
        }
    }

    pub fn activate(
        &self,
        manager: &UnitManager,
        config: &Config,
    ) -> Result<(), UnitActivateError> {
        let mut running = Runny::new(self.description.exec_start.as_str())
            .directory(&Some(config.working_directory(
                &self.description.unit_directory,
                &self.description.working_directory,
            )))
            .start()?;

        // Have stdout and stderr log their output.
        let control_sender = manager.get_control_channel();
        let control_sender_id = self.id().clone();
        let stdout = running.take_output();
        let stderr = running.take_error();
        let thr_sender_id = control_sender_id.clone();
        let thr_sender = control_sender.clone();
        thread::spawn(move || Self::text_read(thr_sender_id, thr_sender, stdout));
        thread::spawn(move || Self::text_read(control_sender_id, control_sender, stderr));

        let control_sender = manager.get_control_channel();
        let control_sender_id = self.id().clone();

        *self.process.borrow_mut() = Some(running);

        // Send some initial configuration to the client.
        control_sender
            .send(ManagerControlMessage::new(
                &control_sender_id,
                ManagerControlMessageContents::InitialGreeting,
            ))
            .ok();

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
        } else {
            Ok(())
        }
    }

    /// Cause a MessageControlContents to be written out.
    pub fn output_message(&self, msg: ManagerStatusMessage) -> Result<(), Error> {
        let mut process_opt = self.process.borrow_mut();

        if process_opt.is_none() {
            return Err(Error::new(ErrorKind::Other, "no process running"));
        }

        let process = process_opt.as_mut().unwrap();

        match msg {
            ManagerStatusMessage::Log(l) => match self.description.format {
                LoggerFormat::Tsv => self.tsv_write(l, process),
                LoggerFormat::Json => self.json_write(l, process),
            },
            _ => Ok(()),
        }
    }

    fn json_write(&self, entry: LogEntry, process: &mut Running) -> Result<(), Error> {
        /*
        let mut object = json::JsonValue::new_object();
        object["message_class"] = msg.message_class.into();
        object["unit_id"] = msg.unit_id.into();
        object["unit_type"] = msg.unit_type.into();
        object["unix_time"] = msg.unix_time.into();
        object["unix_time_nsecs"] = msg.unix_time_nsecs.into();
        object["message"] = log.into();
        writeln!(&mut stdin, "{}", json::stringify(object))
        */
        writeln!(process, "{}", serde_json::to_string(&entry)?)
    }

    fn cfti_escape(msg: &str) -> String {
        msg.replace("\\", "\\\\")
            .replace("\t", "\\t")
            .replace("\n", "\\n")
            .replace("\r", "\\r")
    }

    /// Write a ManagerStatusMessage to a TSV-formatted output.
    fn tsv_write(&self, l: LogEntry, process: &mut Running) -> Result<(), Error> {
        writeln!(
            process,
            "{}\t{}\t{}\t{}\t{}\t{}",
            l.kind().as_str(),
            Self::cfti_escape(l.id().id()),
            Self::cfti_escape(&format!("{}", l.id().kind())),
            l.secs(),
            l.nsecs(),
            Self::cfti_escape(l.message())
        )
    }
}
