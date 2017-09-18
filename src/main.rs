extern crate ctrlc;
extern crate clap;

mod unitbroadcaster;
mod unitwatcher;
mod terminal;

use unitbroadcaster::{UnitEvent, UnitBroadcaster};
use unitwatcher::UnitWatcher;

use clap::{Arg, App};

fn main_loop(unit_broadcaster: &UnitBroadcaster) {
    let rx = unit_broadcaster.subscribe();
    while let Ok(msg) = rx.recv() {
        match msg {
            UnitEvent::Shutdown => return,
            UnitEvent::Status(_) => (),
            UnitEvent::Category(_) => (),
        }
    }
}

fn main() {
    let unit_broadcaster = UnitBroadcaster::new();

    // The signal handler must come first, so that the same mask gets
    // applied to all threads.
    let ctrl_c_broadcaster = unit_broadcaster.clone();
    ctrlc::set_handler(move || {
            ctrl_c_broadcaster.broadcast(&UnitEvent::Shutdown);
        })
        .expect("Error setting Ctrl-C handler");

    let matches = App::new("Exclave Testing System")
        .version("1.0")
        .author("Sean Cross <sean@xobs.io>")
        .about("Orchestrates the Common Factory Test Interface server")
        .arg(Arg::with_name("CONFIG_DIR")
            .short("c")
            .long("config-dir")
            .value_name("CONFIG_DIR")
            .number_of_values(1)
            .required(true)
            .multiple(true)
            .takes_value(true)
            .help("Directory where configuration unit files are stored"))
        .arg(Arg::with_name("PLAIN")
            .short("p")
            .long("plain-output")
            .help("Force output to be 'plain' (rather than auto-detected)"))
        .arg(Arg::with_name("QUIET")
            .short("q")
            .long("no-output")
            .help("Prevent console output entirely"))
        .get_matches();

    let config_dirs: Vec<_> = matches.values_of("CONFIG_DIR").unwrap().collect();
    let output_type = if matches.is_present("PLAIN") {
        Some(terminal::TerminalOutputType::Plain)
    } else if matches.is_present("QUIET") {
        Some(terminal::TerminalOutputType::None)
    } else {
        None
    };

    let mut unit_loader = UnitWatcher::new(&unit_broadcaster);

    terminal::TerminalInterface::start(output_type, unit_broadcaster.subscribe());

    for config_dir in config_dirs {
        unit_loader.add_path(config_dir).expect("Unable to add config directory");
    }

    main_loop(&unit_broadcaster);
}