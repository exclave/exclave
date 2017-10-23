extern crate ctrlc;
extern crate clap;

use std::sync::{Arc, Mutex};
use std::time::Duration;

mod unit;
mod unitbroadcaster;
mod unitlibrary;
mod unitloader;
mod unitmanager;
mod units;
mod unitwatcher;
mod terminal;
mod config;
mod quiesce;

use unitbroadcaster::{UnitEvent, UnitBroadcaster};
use unitwatcher::UnitWatcher;
use unitloader::UnitLoader;
use unitlibrary::UnitLibrary;

use clap::{Arg, App};

fn main() {
    let config = Arc::new(Mutex::new(config::Config::new()));

    let unit_broadcaster = UnitBroadcaster::new();
    let message_receiver = unit_broadcaster.subscribe();
    let mut unit_library = UnitLibrary::new(&unit_broadcaster, &config);
    let unit_loader = UnitLoader::new(&unit_broadcaster);
    let mut unit_watcher = UnitWatcher::new(&unit_broadcaster);

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

    terminal::TerminalInterface::start(output_type, unit_broadcaster.subscribe());

    for config_dir in config_dirs {
        unit_watcher.add_path(config_dir).expect("Unable to add config directory");
    }

    let mut quiesce = quiesce::Quiesce::new(Duration::from_secs(3), &unit_broadcaster);

    use std::fs::File;
    use std::path::Path;
    use std::io::Write;
    let fname = "log.txt";
    let path = Path::new(fname);
    let mut file = File::create(&path).expect("Couldn't create logfile");
    // Main message loop.  Monitor messages and pass them to each component.
    let mut i = 1;
    while let Ok(msg) = message_receiver.recv() {
        writeln!(file, "Got message {}: {:?}", i, msg).expect("Couldn't write message to logfile");
        i = i + 1;
        unit_loader.process_message(&msg);
        unit_library.process_message(&msg);
        quiesce.process_message(&msg);
    }
}