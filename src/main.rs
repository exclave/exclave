extern crate ctrlc;
extern crate clap;

mod unitloader;
mod terminal;

use clap::{Arg, App};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn main() {
    // The signal handler must come first, so that the same mask gets
    // applied to all threads.
    let is_running = Arc::new(AtomicBool::new(true));
    {
        let r = is_running.clone();
        ctrlc::set_handler(move || {
                r.store(false, Ordering::SeqCst);
            })
            .expect("Error setting Ctrl-C handler");
    }

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
        .get_matches();

    let config_dirs: Vec<_> = matches.values_of("CONFIG_DIR").unwrap().collect();
    let output_type = match matches.is_present("PLAIN") {
        true => terminal::TerminalOutputType::Plain,
        false => terminal::TerminalOutputType::Default,
    };

    let mut unit_loader = unitloader::UnitLoader::new();
    terminal::TerminalInterface::start(output_type, unit_loader.subscribe());

    for config_dir in config_dirs {
        println!("Config dir: {}", config_dir);
        unit_loader.add_path(config_dir);
    }

    // Wait for Control-C to be pressed.
    while is_running.load(Ordering::SeqCst) {}
}