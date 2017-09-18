extern crate console;

use self::console::Term;
use unitloader;
use unitloader::{UnitEvent, UnitCategoryEvent, UnitStatusEvent};
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::Receiver;
use std::thread;

#[derive(PartialEq)]
pub enum TerminalOutputType {
    Fancy,
    Plain,
    None,
}

pub struct TerminalInterface {
    /// A list of known categories, and their statuses.
    category_status: HashMap<unitloader::UnitKind, unitloader::UnitCategoryStatus>,

    /// A hashmap of unit types, with each bucket containing a tree of units of statuses.
    /// Each time a status is updated, it is put in its appropriate bucket.
    unit_status: HashMap<unitloader::UnitKind, BTreeMap<unitloader::UnitName, unitloader::UnitStatus>>,

    /// The current stdout of the terminal.
    terminal: Term,

    /// The currently-selected Terminal output (i.e. "Fancy", "Plain", "None", ...).
    output_type: TerminalOutputType,

    /// A list of how many lines was printed during the last fancy print.
    /// Eventually, printers should be moved to their own Trait, and this
    /// will go away.
    last_line_count: u32,
}

impl TerminalInterface {
    pub fn start(output_type: Option<TerminalOutputType>,
                 receiver: Receiver<unitloader::UnitEvent>) {
        let stdout = Term::stdout();
        let output_type = match output_type {
            Some(s) => s,
            None if stdout.is_term() => TerminalOutputType::Fancy,
            None => TerminalOutputType::Plain,
        };

        thread::spawn(move || {
            let mut ti = TerminalInterface {
                output_type: output_type,
                unit_status: HashMap::new(),
                category_status: HashMap::new(),
                terminal: stdout,
                last_line_count: 0,
            };

            while let Ok(event) = receiver.recv() {
                ti.update_unit(event);
            }
            println!("Received error from receiver");
        });
    }

    fn update_unit(&mut self, event: UnitEvent) {

        let mut new_items = 0u32;

        // Insert the new event into the relevent data structures
        match event {
            UnitEvent::Category(ref cat) => {
                if ! self.category_status.contains_key(cat.kind()) {
                    new_items = new_items + 1;
                }
                self.category_status.insert(cat.kind().clone(), cat.status().clone());
            },
            UnitEvent::Status(ref stat) => {
                if ! self.category_status.contains_key(&stat.kind()) {
                    self.category_status.insert(stat.kind().clone(), "".to_owned());
                    new_items = new_items + 1;
                }
                // If this event is for a brand-new unit kind, ensure there is an
                // entry in the unit status map for it.
                if ! self.unit_status.contains_key(stat.kind()) {
                    self.unit_status.insert(stat.kind().clone(), BTreeMap::new());
                }
                self.unit_status.get_mut(stat.kind()).unwrap().insert(stat.name().clone(), stat.status().clone());
            }
        }

        match self.output_type {
            TerminalOutputType::Plain => self.draw_event(event),
            TerminalOutputType::Fancy => self.redraw_screen(event, new_items),
            TerminalOutputType::None => (),
        };
    }

    fn draw_event(&self, event: UnitEvent) {
        match event {
            UnitEvent::Status(stat) => println!("    {} -> {}", stat.name(), stat.status()),
            UnitEvent::Category(stat) => println!("{}: {}", stat.kind(), stat.status()),
        };
    }

    fn redraw_screen(&mut self, event: UnitEvent, new_items: u32) {
        /*
        // Clear out the previous entries, plus the headers for the field types.
        self.terminal.clear_last_lines(self.unit_status.len() + 3).expect("Unable to clear lines");

        self.terminal.write_line(format!("{}", console::style("Jigs:").bold()).as_str()).expect("Unable to write jig header");
        for (event_name, event) in &self.unit_status {
            if event_name.kind() != &unitloader::UnitKind::Jig {
                continue;
            }
            self.terminal.write_line(format!("    {}: {}",
                                             console::style(event_name).green(),
                                             console::style(event).yellow())
                .as_str()).expect("Unable to write jig");
        }

        self.terminal.write_line(format!("{}", console::style("Scenarios:").bold()).as_str()).expect("Unable to write scenario header");
        for (event_name, event) in &self.unit_status {
            if event_name.kind() != &unitloader::UnitKind::Scenario {
                continue;
            }
            self.terminal.write_line(format!("    {}: {}",
                                             console::style(event_name).green(),
                                             console::style(event).yellow())
                .as_str()).expect("Unable to write scenario");
        }

        self.terminal.write_line(format!("{}", console::style("Tests:").bold()).as_str()).expect("Unable to write test header");
        for (event_name, event) in &self.unit_status {
            if event_name.kind() != &unitloader::UnitKind::Test {
                continue;
            }
            self.terminal.write_line(format!("    {}: {}",
                                             console::style(event_name).green(),
                                             console::style(event).yellow())
                .as_str()).expect("Unable to write test");
        }
        */
    }
}