extern crate ctrlc;
extern crate clap;

use std::sync::{Arc, Mutex};

mod unit;
mod unitbroadcaster;
mod unitlibrary;
mod unitloader;
mod unitmanager;
mod units;
mod unitwatcher;
mod terminal;
mod config;

use unitbroadcaster::{UnitEvent, UnitBroadcaster};
use unitwatcher::UnitWatcher;
use unitloader::UnitLoader;
use unitlibrary::UnitLibrary;

use clap::{Arg, App};

fn main() {
    let config = Arc::new(Mutex::new(config::Config::new()));

    let unit_broadcaster = UnitBroadcaster::new();
    let unit_library = Arc::new(Mutex::new(UnitLibrary::new(&unit_broadcaster, &config)));
    let unit_loader = UnitLoader::new(&unit_broadcaster, &unit_library);

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

    let mut unit_watcher = UnitWatcher::new(&unit_broadcaster);

    terminal::TerminalInterface::start(output_type, unit_broadcaster.subscribe());

    for config_dir in config_dirs {
        unit_watcher.add_path(config_dir).expect("Unable to add config directory");
    }

    unit_loader.process_messages();
}