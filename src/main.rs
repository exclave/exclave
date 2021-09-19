extern crate clap;
extern crate ctrlc;

#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;

use std::sync::{Arc, Mutex};
use std::time::Duration;

mod config;
mod quiesce;
mod terminal;
mod unit;
mod unitbroadcaster;
mod unitlibrary;
mod unitloader;
mod unitmanager;
mod units;
mod unitwatcher;

use unitbroadcaster::{UnitBroadcaster, UnitEvent};
use unitlibrary::UnitLibrary;
use unitloader::UnitLoader;
use unitwatcher::UnitWatcher;

use clap::{App, Arg};

fn main() {
    let config = Arc::new(Mutex::new(config::Config::new()));

    let unit_broadcaster = UnitBroadcaster::new();
    let message_receiver = unit_broadcaster.subscribe();
    let unit_library = UnitLibrary::new(&unit_broadcaster, &config);
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
        .version(env!("CARGO_PKG_VERSION"))
        .long_version(env!("GIT_VERSION"))
        .author("Sean Cross <sean@xobs.io>")
        .about("Orchestrates the Common Factory Test Interface server")
        .arg(
            Arg::with_name("CONFIG_DIR")
                .short("c")
                .long("config-dir")
                .value_name("CONFIG_DIR")
                .number_of_values(1)
                .required(true)
                .multiple(true)
                .takes_value(true)
                .help("Directory where configuration unit files are stored"),
        )
        .arg(
            Arg::with_name("PLAIN")
                .short("p")
                .long("plain-output")
                .help("Force output to be 'plain' (rather than auto-detected)"),
        )
        .arg(
            Arg::with_name("QUIET")
                .short("q")
                .long("no-output")
                .help("Prevent console output entirely"),
        )
        .arg(
            Arg::with_name("DEBUG_LOGFILE")
                .short("9")
                .long("debug-log")
                .help("Log all internal messages to the specified file")
                .required(false)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("KEYBOARD_TRIGGER")
                .short("k")
                .long("keyboard-trigger")
                .help("Run default scenario on enter key press"),
        )
        .get_matches();

    let config_dirs: Vec<_> = matches.values_of("CONFIG_DIR").unwrap().collect();
    let output_type = if matches.is_present("PLAIN") {
        Some(terminal::TerminalOutputType::Plain)
    } else if matches.is_present("QUIET") {
        Some(terminal::TerminalOutputType::None)
    } else {
        None
    };

    terminal::TerminalInterface::start(
        output_type,
        &unit_broadcaster,
        matches.is_present("KEYBOARD_TRIGGER"),
    );

    for config_dir in config_dirs {
        unit_watcher
            .add_path(config_dir)
            .unwrap_or_else(|_| panic!("Unable to add config directory {}", config_dir));
    }

    let mut quiesce = quiesce::Quiesce::new(Duration::from_secs(1), &unit_broadcaster);

    unit_broadcaster.log("main", "Exclave initializing".to_string());

    let mut debug_file = match matches.value_of("DEBUG_LOGFILE") {
        None => None,
        Some(dv) => {
            use std::fs::File;
            use std::path::Path;
            let path = Path::new(dv);
            Some(File::create(&path).expect("Couldn't create logfile"))
        }
    };
    // Main message loop.  Monitor messages and pass them to each component.
    let mut loops = 1;
    while let Ok(msg) = message_receiver.recv() {
        if let Some(file) = debug_file.as_mut() {
            use std::io::Write;
            use std::time;

            let now = match time::SystemTime::now().duration_since(time::UNIX_EPOCH) {
                Ok(d) => d,
                Err(_) => time::Duration::new(0, 0),
            };

            let unix_time = now.as_secs();
            let unix_time_nsecs = now.subsec_nanos();

            writeln!(
                file,
                "{}:{}.{} {:?}",
                loops, unix_time, unix_time_nsecs, msg
            )
            .expect("Couldn't write message to logfile");
        }
        loops += 1;
        unit_loader.process_message(&msg);
        unit_library.process_message(&msg);
        quiesce.process_message(&msg);
    }
}

#[cfg(test)]
mod test;
