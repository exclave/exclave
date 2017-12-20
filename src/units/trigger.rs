extern crate runny;
extern crate systemd_parser;

use std::cell::RefCell;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;

use config::Config;
use unit::{UnitActivateError, UnitDeactivateError, UnitDescriptionError, UnitIncompatibleReason, UnitSelectError, UnitDeselectError,
           UnitName};
use unitmanager::{ManagerControlMessage, ManagerControlMessageContents, UnitManager};

use self::systemd_parser::items::DirectiveEntry;
use self::runny::Runny;
use self::runny::running::{Running, RunningOutput};

#[derive(Clone, Copy)]
enum TriggerFormat {
    Text,
    JSON,
}

/// A struct defining an in-memory representation of a .Trigger file
#[derive(Clone)]
pub struct TriggerDescription {
    /// The id of the unit (including the kind)
    id: UnitName,

    /// A short name
    name: String,

    /// A detailed description of this Trigger, up to one paragraph.
    description: String,

    /// A Vec<String> of jig names that this test is compatible with.
    jigs: Vec<UnitName>,

    /// Path to the command to start the Trigger
    exec_start: String,

    /// The format expected by the Trigger
    format: TriggerFormat,

    /// The working directory to start from when running the Trigger
    working_directory: Option<PathBuf>,
}

impl TriggerDescription {
    pub fn from_path(path: &Path) -> Result<TriggerDescription, UnitDescriptionError> {
        let unit_name = UnitName::from_path(path)?;

        // Parse the file into a systemd unit_file object
        let mut contents = String::with_capacity(8192);
        File::open(path)?.read_to_string(&mut contents)?;
        let unit_file = systemd_parser::parse_string(&contents)?;

        if !unit_file.has_category("Trigger") {
            return Err(UnitDescriptionError::MissingSection("Trigger".to_owned()));
        }

        let mut interface_description = TriggerDescription {
            id: unit_name,
            name: "".to_owned(),
            description: "".to_owned(),
            jigs: vec![],
            format: TriggerFormat::Text,
            exec_start: "".to_owned(),
            working_directory: None,
        };

        for entry in unit_file.lookup_by_category("Trigger") {
            match entry {
                &DirectiveEntry::Solo(ref directive) => match directive.key() {
                    "Name" => {
                        interface_description.name = directive.value().unwrap_or("").to_owned()
                    }
                    "Description" => {
                        interface_description.description =
                            directive.value().unwrap_or("").to_owned()
                    }
                    "Jigs" => {
                        interface_description.jigs = match directive.value() {
                            Some(s) => UnitName::from_list(s, "jig")?,
                            None => vec![],
                        }
                    }
                    "WorkingDirectory" => {
                        interface_description.working_directory =
                            Some(Path::new(directive.value().unwrap_or("")).to_owned())
                    }
                    "ExecStart" => {
                        interface_description.exec_start = match directive.value() {
                            Some(s) => s.to_owned(),
                            None => {
                                return Err(UnitDescriptionError::MissingValue(
                                    "Trigger".to_owned(),
                                    "ExecStart".to_owned(),
                                ))
                            }
                        }
                    }
                    "Format" => {
                        interface_description.format = match directive.value() {
                            None => TriggerFormat::Text,
                            Some(s) => match s.to_string().to_lowercase().as_ref() {
                                "text" => TriggerFormat::Text,
                                "json" => TriggerFormat::JSON,
                                other => {
                                    return Err(UnitDescriptionError::InvalidValue(
                                        "Trigger".to_owned(),
                                        "Format".to_owned(),
                                        other.to_owned(),
                                        vec!["text".to_owned(), "json".to_owned()],
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
        Ok(interface_description)
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
    ) -> Result<Trigger, UnitIncompatibleReason> {
        self.is_compatible(manager, config)?;

        Ok(Trigger::new(self, manager, config))
    }
}

pub struct Trigger {
    description: TriggerDescription,
    process: RefCell<Option<Running>>,
}

impl Trigger {
    pub fn new(desc: &TriggerDescription, _: &UnitManager, _: &Config) -> Trigger {
        Trigger {
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

        let stdout = running.take_output();
        let stderr = running.take_error();

        let control_sender = manager.get_control_channel();
        let control_sender_id = self.id().clone();
        match self.description.format {
            TriggerFormat::Text => {
                // Pass control to an out-of-object thread, and shuttle communications
                // from stdout onto the control_sender channel.
                let thr_sender_id = control_sender_id.clone();
                let thr_sender = control_sender.clone();
                thread::spawn(move || Self::text_read(thr_sender_id, thr_sender, stdout));
                let thr_sender_id = control_sender_id.clone();
                let thr_sender = control_sender.clone();
                thread::spawn(move || Self::text_read_stderr(thr_sender_id, thr_sender, stderr));
            }
            TriggerFormat::JSON => {
                ();
            }
        };

        *self.process.borrow_mut() = Some(running);

        // Send some initial configuration to the client.
        control_sender.send(ManagerControlMessage::new(&control_sender_id, ManagerControlMessageContents::InitialGreeting)).ok();

        Ok(())
    }

    pub fn deactivate(&self) -> Result<(), UnitDeactivateError> {
        if let Some(process) = self.process.borrow_mut().take() {
            match process.terminate(None) {
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

    fn cfti_unescape(msg: String) -> String {
        msg.replace("\\t", "\t")
            .replace("\\n", "\n")
            .replace("\\r", "\r")
            .replace("\\\\", "\\")
    }

    fn text_read_stderr(id: UnitName, control: Sender<ManagerControlMessage>, output: RunningOutput) {
        for line in BufReader::new(output).lines() {
            let line = line.expect("Unable to get next line");
            // If the send fails, that means the other end has closed the pipe.
            if let Err(_) = control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::LogError(line))) {
                break;
            }
        }
    }

    fn text_read(id: UnitName, control: Sender<ManagerControlMessage>, stdout: RunningOutput) {
        for line in BufReader::new(stdout).lines() {
            let line = line.expect("Unable to get next line");
            let mut words: Vec<String> = line.split_whitespace()
                .map(|x| Self::cfti_unescape(x.to_owned()))
                .collect();

            // Don't crash if we get a blank line.
            let msg = if words.len() == 0 {
                ManagerControlMessageContents::StartScenario(None)
            }
            else {

                let verb = words[0].to_lowercase();
                words.remove(0);

                match verb.as_str() {
                    "stop" => ManagerControlMessageContents::Unimplemented("stop".to_owned(), "Unable to stop tests".to_owned()),
                    "start" => {
                        if words.len() > 0 {
                            match UnitName::from_str(&words[0], "test") {
                                Ok(name) => ManagerControlMessageContents::StartScenario(Some(name)),
                                Err(_) => ManagerControlMessageContents::Unimplemented(words[0].clone(), "name could not be decoded".to_owned()),
                            }
                        } else {
                            ManagerControlMessageContents::StartScenario(None)
                        }
                    },
                    v => ManagerControlMessageContents::Unimplemented(v.to_owned(), words.join(" ")),
                }
            };

            // If the send fails, that means the other end has closed the pipe.
            if let Err(_) = control.send(ManagerControlMessage::new(&id, msg)) {
                break;
            }
        }
        control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::ChildExited)).expect("interface couldn't send exit message to controller");
    }
}
