extern crate console;

use self::console::{Term, Style, style};
use unitloader;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::thread;

#[derive(PartialEq)]
pub enum TerminalOutputType {
    Default,
    Fancy,
    Plain,
    None,
}

pub struct TerminalInterface {
    output_type: TerminalOutputType,
    unit_status: HashMap<unitloader::UnitName, unitloader::UnitStatus>,
    terminal: Term,
    style: Style,
}

impl TerminalInterface {
    pub fn start(output_type: TerminalOutputType,
                 receiver: Receiver<unitloader::UnitStatusEvent>) {
        let stdout = Term::stdout();
        let output_type = if output_type == TerminalOutputType::Default && stdout.is_term() {
            TerminalOutputType::Fancy
        } else {
            output_type
        };

        thread::spawn(move || {
            let mut ti = TerminalInterface {
                output_type: output_type,
                unit_status: HashMap::new(),
                terminal: stdout,
                style: Style::new(),
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
            _ => (),
        };
    }

    fn redraw_screen(&mut self, event: unitloader::UnitStatusEvent) {
        // Clear out the previous entries, plus the headers for the field types.
        self.terminal.clear_last_lines(self.unit_status.len() + 3);
        self.unit_status.insert(event.name, event.status);

        self.terminal.write_line(format!("{}", console::style("Jigs:").bold()).as_str());
        for (event_name, event) in &self.unit_status {
            if event_name.kind() != &unitloader::UnitKind::Jig {
                continue;
            }
            self.terminal.write_line(format!("    {}: {}",
                                             console::style(event_name).green(),
                                             console::style(event).yellow())
                .as_str());
        }

        self.terminal.write_line(format!("{}", console::style("Scenarios:").bold()).as_str());
        for (event_name, event) in &self.unit_status {
            if event_name.kind() != &unitloader::UnitKind::Scenario {
                continue;
            }
            self.terminal.write_line(format!("    {}: {}",
                                             console::style(event_name).green(),
                                             console::style(event).yellow())
                .as_str());
        }

        self.terminal.write_line(format!("{}", console::style("Tests:").bold()).as_str());
        for (event_name, event) in &self.unit_status {
            if event_name.kind() != &unitloader::UnitKind::Test {
                continue;
            }
            self.terminal.write_line(format!("    {}: {}",
                                             console::style(event_name).green(),
                                             console::style(event).yellow())
                .as_str());
        }
    }
}