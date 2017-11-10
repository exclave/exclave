extern crate runny;
extern crate systemd_parser;

use std::cell::RefCell;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write, Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use config::Config;
use unit::{UnitActivateError, UnitDeactivateError, UnitDescriptionError, UnitIncompatibleReason, UnitSelectError, UnitDeselectError,
           UnitName};
use unitmanager::{ManagerControlMessage, ManagerControlMessageContents, ManagerStatusMessage,
                  UnitManager};

use self::systemd_parser::items::DirectiveEntry;
use self::runny::Runny;
use self::runny::running::{Running, RunningOutput};

#[derive(Clone, Copy)]
enum InterfaceFormat {
    Text,
    JSON,
}

/// A struct defining an in-memory representation of a .Interface file
pub struct InterfaceDescription {
    /// The id of the unit (including the kind)
    id: UnitName,

    /// A short name
    name: String,

    /// A detailed description of this Interface, up to one paragraph.
    description: String,

    /// A Vec<String> of jig names that this test is compatible with.
    jigs: Vec<UnitName>,

    /// Path to the command to start the interface
    exec_start: String,

    /// The format expected by the interface
    format: InterfaceFormat,

    /// The working directory to start from when running the interface
    working_directory: Option<PathBuf>,
}

impl InterfaceDescription {
    pub fn from_path(path: &Path) -> Result<InterfaceDescription, UnitDescriptionError> {
        let unit_name = UnitName::from_path(path)?;

        // Parse the file into a systemd unit_file object
        let mut contents = String::with_capacity(8192);
        File::open(path)?.read_to_string(&mut contents)?;
        let unit_file = systemd_parser::parse_string(&contents)?;

        if !unit_file.has_category("Interface") {
            return Err(UnitDescriptionError::MissingSection("Interface".to_owned()));
        }

        let mut interface_description = InterfaceDescription {
            id: unit_name,
            name: "".to_owned(),
            description: "".to_owned(),
            jigs: vec![],
            format: InterfaceFormat::Text,
            exec_start: "".to_owned(),
            working_directory: None,
        };

        for entry in unit_file.lookup_by_category("Interface") {
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
                                    "Interface".to_owned(),
                                    "ExecStart".to_owned(),
                                ))
                            }
                        }
                    }
                    "Format" => {
                        interface_description.format = match directive.value() {
                            None => InterfaceFormat::Text,
                            Some(s) => match s.to_string().to_lowercase().as_ref() {
                                "text" => InterfaceFormat::Text,
                                "json" => InterfaceFormat::JSON,
                                other => {
                                    return Err(UnitDescriptionError::InvalidValue(
                                        "Interface".to_owned(),
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
    ) -> Result<Interface, UnitIncompatibleReason> {
        self.is_compatible(manager, config)?;

        Ok(Interface::new(self, manager, config))
    }
}

pub struct Interface {
    id: UnitName,
    exec_start: String,
    working_directory: Option<PathBuf>,
    format: InterfaceFormat,
    process: RefCell<Option<Running>>,
    terminate_timeout: Duration,
}

impl Interface {
    pub fn new(desc: &InterfaceDescription, _: &UnitManager, config: &Config) -> Interface {
        Interface {
            id: desc.id.clone(),
            exec_start: desc.exec_start.clone(),
            working_directory: desc.working_directory.clone(),
            format: desc.format,
            process: RefCell::new(None),
            terminate_timeout: config.terminate_timeout().clone(),
        }
    }

    pub fn id(&self) -> &UnitName {
        &self.id
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
        let mut running = Runny::new(self.exec_start.as_str())
                    .directory(&Some(config.working_directory(&self.working_directory)))
                    .start()?;

        let stdout = running.take_output();
        let stderr = running.take_error();

        let control_sender = manager.get_control_channel();
        let control_sender_id = self.id().clone();
        match self.format {
            InterfaceFormat::Text => {
                // Pass control to an out-of-object thread, and shuttle communications
                // from stdout onto the control_sender channel.
                let thr_sender_id = control_sender_id.clone();
                let thr_sender = control_sender.clone();
                thread::spawn(move || Self::text_read(thr_sender_id, thr_sender, stdout));
                let thr_sender_id = control_sender_id.clone();
                let thr_sender = control_sender.clone();
                thread::spawn(move || Self::text_read_stderr(thr_sender_id, thr_sender, stderr));
            }
            InterfaceFormat::JSON => {
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
            match process.terminate(Some(self.terminate_timeout)) {
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
        match self.format {
            InterfaceFormat::Text => self.text_write(msg),
            InterfaceFormat::JSON => self.json_write(msg),
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

    /// Write a UnitInterfaceMessage to a Text-formatted output.
    fn text_write(&self, msg: ManagerStatusMessage) -> Result<(), Error> {
        let mut process_opt = self.process.borrow_mut();

        if process_opt.is_none() {
            return Err(Error::new(ErrorKind::Other, "no process running"));
        }

        let process = process_opt.as_mut().unwrap();

        match msg {
            ManagerStatusMessage::Jig(j) => match j {
                Some(jig_name) => writeln!(process, "JIG {}", Self::cfti_escape(&format!("{}", jig_name))),
                None => writeln!(process, "JIG"),
            },
            ManagerStatusMessage::Hello(id) => writeln!(process, "HELLO {}", Self::cfti_escape(&format!("{}", id))),
            ManagerStatusMessage::Tests(scenario, tests) => {
                write!(process, "TESTS {}", Self::cfti_escape(scenario.id()))?;
                for test in &tests {
                    write!(process, " {}", Self::cfti_escape(test.id()))?;
                }
                writeln!(process, "")
            },
            ManagerStatusMessage::Scenario(name) => match name {
                Some(s) => writeln!(process, "SCENARIO {}", Self::cfti_escape(s.id())),
                None => writeln!(process, "SCENARIO"),
            },
            ManagerStatusMessage::Scenarios(list) => {
                write!(process, "SCENARIOS")?;
                for scenario_name in list {
                    write!(process, " {}", Self::cfti_escape(scenario_name.id()))?;
                }
                writeln!(process, "")
            },
            ManagerStatusMessage::Describe(id, field, value) => {
                writeln!(process, "DESCRIBE {}", Self::cfti_escape(&format!("{} {} {} {}", id.kind(), field, id.id(), value)))
            }
            ManagerStatusMessage::Log(l) => writeln!(
                process,
                "LOG {}\t{}\t{}\t{}\t{}\t{}",
                l.kind().as_str(),
                l.id().id(),
                l.id().kind(),
                l.secs(),
                l.nsecs(),
                l.message()
                    .replace("\\", "\\\\")
                    .replace("\t", "\\t")
                    .replace("\n", "\\n")
                    .replace("\r", "\\r")
            ),
            ManagerStatusMessage::Running(test) => writeln!(process, "RUNNING {}", test.id()),
            ManagerStatusMessage::Skipped(test, reason) => {
                writeln!(process, "SKIP {} {}", test.id(), reason)
            },
            ManagerStatusMessage::Finished(scenario, result, reason) => {
                writeln!(process, "FINISH {} {} {}", scenario.id(), result, reason)
            },
            ManagerStatusMessage::Fail(test, _code, reason) => {
                writeln!(process, "FAIL {} {}", test.id(), reason)
            }
            ManagerStatusMessage::Pass(test, reason) => {
                writeln!(process, "PASS {} {}", test.id(), reason)
            }             /*
            //            BroadcastMessageContents::Hello(name) => writeln!(stdin,
            //                                                "HELLO {}", name),
            //            BroadcastMessageContents::Ping(val) => writeln!(stdin,
            //                                                "PING {}", val),
            BroadcastMessageContents::Shutdown(reason) => writeln!(stdin, "EXIT {}", reason),

            BroadcastMessageContents::Start(scenario) => writeln!(stdin, "START {}", scenario),
            */
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
            if words.len() == 0 {
                continue;
            }

            let verb = words[0].to_lowercase();
            words.remove(0);

            let response = match verb.as_str() {
                "scenarios" => ManagerControlMessageContents::Scenarios,
                "scenario" => match UnitName::from_str(words.get(0).unwrap_or(&"".to_owned()).to_lowercase().as_str(), "scenario") {
                        Err(e) => ManagerControlMessageContents::Error(format!("Invalid scenario name: {}", e)),
                        Ok(o) => ManagerControlMessageContents::Scenario(o),
                    }
                ,
                "tests" => {
                    if words.is_empty() {
                        ManagerControlMessageContents::Tests(None)
                    } else {
                        match UnitName::from_str(words[0].to_lowercase().as_str(), "test") {
                            Ok(scenario_name) => ManagerControlMessageContents::Tests(Some(scenario_name)),
                            Err(e) => ManagerControlMessageContents::Error(format!("Invalid test name specified: {}", e)),
                        }
                    }
                },
                "jig" => ManagerControlMessageContents::Jig,
                "log" => ManagerControlMessageContents::Log(words.join(" ")),
                "start" => {
                    if words.is_empty() {
                        ManagerControlMessageContents::StartScenario(None)
                    } else {
                        match UnitName::from_str(words.get(0).unwrap_or(&"".to_owned()).to_lowercase().as_str(), "scenario") {
                            Err(e) => ManagerControlMessageContents::Error(format!("Invalid scenario name: {}", e)),
                            Ok(o) => ManagerControlMessageContents::StartScenario(Some(o)),
                        }
                    }
                }
                /*
                "abort" => ControlMessageContents::AbortTests,
                "pong" => ControlMessageContents::Pong(words[0].to_lowercase()),
                "hello" => ControlMessageContents::Hello(words.join(" ")),
                "shutdown" => {
                    if words.is_empty() {
                        ControlMessageContents::Shutdown(None)
                    } else {
                        ControlMessageContents::Shutdown(Some(words.join(" ")))
                    }
                }
                */
                v => ManagerControlMessageContents::Unimplemented(v.to_owned(), words.join(" ")),
            };

            // If the send fails, that means the other end has closed the pipe.
            if let Err(_) = control.send(ManagerControlMessage::new(&id, response)) {
                break;
            }
        }
        control.send(ManagerControlMessage::new(&id, ManagerControlMessageContents::ChildExited)).expect("interface couldn't send exit message to controller");
    }
}
