extern crate console;

use self::console::Term;
use unit::{UnitKind, UnitName};
use unitbroadcaster::{LogEntry, UnitCategoryStatus, UnitEvent, UnitStatus};
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::Receiver;
use unitbroadcaster::{UnitBroadcaster};
use std::thread;
use unitmanager::{ManagerControlMessage, ManagerControlMessageContents};

#[derive(PartialEq)]
pub enum TerminalOutputType {
    Fancy,
    Plain,
    None,
}

const MAX_LOG_HISTORY: usize = 25;

pub struct TerminalInterface {
    /// A list of known categories, and their statuses.
    category_status: BTreeMap<UnitKind, UnitCategoryStatus>,

    /// A hashmap of unit types, with each bucket containing a tree of units of statuses.
    /// Each time a status is updated, it is put in its appropriate bucket.
    unit_status: HashMap<UnitKind, BTreeMap<UnitName, UnitStatus>>,

    /// A hashmap of the last few log entries
    logs: Vec<LogEntry>,

    /// The current stdout of the terminal.
    terminal: Term,

    /// The currently-selected Terminal output (i.e. "Fancy", "Plain", "None", ...).
    output_type: TerminalOutputType,

    /// A list of how many lines was printed during the last fancy print.
    /// Eventually, printers should be moved to their own Trait, and this
    /// will go away.
    last_line_count: usize,

    /// How many lines of history to keep.
    log_history: usize,
}

impl TerminalInterface {
    pub fn start(output_type: Option<TerminalOutputType>, broadcaster: &UnitBroadcaster, monitor_keypress: bool) {
        let stdout = Term::stdout();
        let output_type = match output_type {
            Some(s) => s,
            None if stdout.is_term() => TerminalOutputType::Fancy,
            None => TerminalOutputType::Plain,
        };
        let receiver = broadcaster.subscribe();

        thread::spawn(move || {
            let mut ti = TerminalInterface {
                output_type: output_type,
                unit_status: HashMap::new(),
                category_status: BTreeMap::new(),
                terminal: stdout,
                last_line_count: 0,
                logs: vec![],
                log_history: MAX_LOG_HISTORY,
            };

            while let Ok(event) = receiver.recv() {
                ti.update_unit(event);
            }
            eprintln!("Receiver has closed -- shutting down");
        });

        if monitor_keypress {
            let id = UnitName::internal("terminal");
            let thread_broadcaster = broadcaster.clone();

            // Broadcast a start scenario message if an enter key is pressed in the terminal where exclave is running
            // Could possibly be extended to do things like, run test #1 when the '1' key is entered or print stats when
            // the '?' key is entered
            thread::spawn(move || {
                loop {
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line).expect("Failed to read line");
                    thread_broadcaster.broadcast(&UnitEvent::ManagerRequest(ManagerControlMessage::new(&id, ManagerControlMessageContents::StartScenario(None))));
                }
            });
        }
    }

    fn update_unit(&mut self, event: UnitEvent) {
        // Insert the new event into the relevent data structures
        match event {
            UnitEvent::Category(ref cat) => {
                self.category_status
                    .insert(cat.kind().clone(), cat.status().clone());
            }
            UnitEvent::Status(ref stat) => {
                if !self.category_status.contains_key(&stat.kind()) {
                    self.category_status
                        .insert(stat.kind().clone(), "".to_owned());
                }
                // If this event is for a brand-new unit kind, ensure there is an
                // entry in the unit status map for it.
                if !self.unit_status.contains_key(stat.kind()) {
                    self.unit_status
                        .insert(stat.kind().clone(), BTreeMap::new());
                }
                self.unit_status
                    .get_mut(stat.kind())
                    .unwrap()
                    .insert(stat.name().clone(), stat.status().clone());
            }
            UnitEvent::Log(ref log) => {
                // Ensure we have a vec for the logs to be stored.
                self.logs.push(log.clone());
                if self.logs.len() > self.log_history {
                    self.logs.remove(0);
                }
            }
            UnitEvent::RescanStart => (),
            UnitEvent::RescanFinish => (),
            UnitEvent::RescanRequest => (),
            UnitEvent::Shutdown => (),
            UnitEvent::ManagerRequest(_) => (),
        }

        match self.output_type {
            TerminalOutputType::Plain => self.draw_event(event),
            TerminalOutputType::Fancy => self.redraw_screen(event),
            TerminalOutputType::None => (),
        };
    }

    fn draw_event(&self, event: UnitEvent) {
        match event {
            UnitEvent::Status(stat) => println!("    {} -> {}", stat.name(), stat.status()),
            UnitEvent::Category(stat) => println!("{}: {}", stat.kind(), stat.status()),
            UnitEvent::RescanRequest => println!("Unit rescan requested"),
            UnitEvent::RescanStart => println!("Started unit recsan..."),
            UnitEvent::RescanFinish => println!("Finished rescanning units"),
            UnitEvent::Shutdown => println!("Shutting down"),
            UnitEvent::Log(log) => println!("{}", log),
            UnitEvent::ManagerRequest(_) => (),
        };
    }

    fn redraw_screen(&mut self, evt: UnitEvent) {
        if evt != UnitEvent::RescanFinish {
            return;
        }

        // Clear out the previous entries, plus the headers for the field types.
        self.terminal
            .clear_last_lines(self.last_line_count)
            .expect("Unable to clear lines");

        let mut line_count = 0;

        for (category_type, category_status) in &self.category_status {
            self.terminal
                .write_line(
                    format!(
                        "{}: {}",
                        console::style(category_type).bold(),
                        console::style(category_status).bold()
                    ).as_str(),
                )
                .expect("Unable to write unit header");
            line_count = line_count + 1;

            for (unit_name, unit_event) in self.unit_status
                .get(&category_type)
                .expect("Couldn't find any category bucket")
                .iter()
            {
                line_count = line_count + 1;
                self.terminal
                    .write_line(
                        format!(
                            "    {}: {}",
                            console::style(unit_name).green(),
                            console::style(unit_event).yellow()
                        ).as_str(),
                    )
                    .expect("Unable to write unit");
            }
        }

        if self.logs.len() > 0 {
            line_count = line_count + 1;
            self.terminal.write_line(format!("Logs: ").as_str()).expect("Unable to write log");
        }
        for log_line in self.logs.iter() {
            line_count = line_count + 1;
            self.terminal.write_line(format!("  {}", log_line).as_str()).expect("Unable to write log");
        }
        self.terminal.flush().expect("Couldn't redraw screen");
        self.last_line_count = line_count;
    }
}
