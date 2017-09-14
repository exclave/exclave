extern crate console;

use self::console::Term;
use unitloader;
use std::collections::BTreeMap;
use std::sync::mpsc::Receiver;
use std::thread;

#[derive(PartialEq)]
pub enum TerminalOutputType {
    Fancy,
    Plain,
    None,
}

pub struct TerminalInterface {
    output_type: TerminalOutputType,
    unit_status: BTreeMap<unitloader::UnitName, unitloader::UnitStatus>,
    terminal: Term,
}

impl TerminalInterface {
    pub fn start(output_type: Option<TerminalOutputType>,
                 receiver: Receiver<unitloader::UnitStatusEvent>) {
        let stdout = Term::stdout();
        let output_type = match output_type {
            Some(s) => s,
            None if stdout.is_term() => TerminalOutputType::Fancy,
            None => TerminalOutputType::Plain,
        };

        thread::spawn(move || {
            let mut ti = TerminalInterface {
                output_type: output_type,
                unit_status: BTreeMap::new(),
                terminal: stdout,
            };

            while let Ok(event) = receiver.recv() {
                ti.update_unit(event);
            }
            println!("Received error from receiver");
        });
    }

    fn update_unit(&mut self, event: unitloader::UnitStatusEvent) {
        match self.output_type {
            TerminalOutputType::Plain => println!("{} -> {}", event.name, event.status),
            TerminalOutputType::Fancy => self.redraw_screen(event),
            TerminalOutputType::None => (),
        };
    }

    fn redraw_screen(&mut self, event: unitloader::UnitStatusEvent) {
        // Clear out the previous entries, plus the headers for the field types.
        self.terminal.clear_last_lines(self.unit_status.len() + 3).expect("Unable to clear lines");
        self.unit_status.insert(event.name, event.status);

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
    }
}