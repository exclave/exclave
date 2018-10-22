use std::sync::{Arc, Mutex};
use std::sync::mpsc::Receiver;
use std::time::Duration;
use std::thread;
use std::path::PathBuf;

use config::Config;
use unit::{UnitKind, UnitName, UnitDescriptionError};
use unitbroadcaster::{UnitBroadcaster, UnitEvent};
//use unitwatcher::UnitWatcher;
//use unitloader::UnitLoader;
use unitmanager::UnitManager;
use units::interface::{Interface, InterfaceDescription};
use units::jig::{Jig, JigDescription};
use units::logger::{Logger, LoggerDescription};
use units::scenario::{Scenario, ScenarioDescription};
use units::test::{Test, TestDescription};
use units::trigger::{Trigger, TriggerDescription};

struct Exclave {
    config: Arc<Mutex<Config>>,
    broadcaster: UnitBroadcaster,
    receiver: Receiver<UnitEvent>,
    manager: UnitManager,
}

const LINUX_JIG: &str = r##"
[Jig]
Name=Linux Jig
Description=Development Jig running on Linux
TestFile=/etc
DefaultScenario=linux-tests
DefaultWorkingDirectory=lintests
"##;

const GENERIC_JIG: &str = r##"
[Jig]
Name=Generic Jig
Description=Generic, all-purpose jig
"##;

fn setup_exclave(timeout: Option<Duration>) -> Exclave {
    let config = Arc::new(Mutex::new(Config::new()));

    let broadcaster = UnitBroadcaster::new();
    let receiver = broadcaster.subscribe();
    let manager = UnitManager::new(&broadcaster, &config);
//    let mut unit_library = UnitLibrary::new(&unit_broadcaster, &config);
//    let unit_loader = UnitLoader::new(&unit_broadcaster);
//    let mut unit_watcher = UnitWatcher::new(&unit_broadcaster);

    // If a timeout is specified, set a maximum time for this test to run.
    if let Some(t) = timeout {
        let timeout_broadcaster = broadcaster.clone();
        thread::spawn(move || {
            thread::sleep(t);
            timeout_broadcaster.broadcast(&UnitEvent::Shutdown);
        });
    }

    Exclave {
        config: config,
        broadcaster: broadcaster,
        receiver: receiver,
        manager: manager,
    }
}

fn add_unit(exclave: &Exclave, name: UnitName, unit_text: &str) -> Result<(), UnitDescriptionError> {
    match *name.kind() {
        UnitKind::Test => {
            let desc = TestDescription::from_string(unit_text, name, &PathBuf::from("test/config"))?;
            exclave.manager.load_test(&desc).unwrap();
        },
        UnitKind::Jig => {
            let desc = JigDescription::from_string(unit_text, name, &PathBuf::from("test/config"))?;
            exclave.manager.load_jig(&desc).unwrap();
        },
        _ => (),
    }
    Ok(())
}

#[test]
fn load_dependency() {
    let exclave = setup_exclave(None);
    add_unit(&exclave, UnitName::from_str("generic", "jig").unwrap(), GENERIC_JIG).ok();
    //add_unit(&exclave, UnitName::from_str("linux", "jig").unwrap(), LINUX_JIG).ok();
    assert!(exclave.manager.jig_is_loaded(&UnitName::from_str("generic", "jig").unwrap()));
}
