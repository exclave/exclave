use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use config::Config;
use unit::{UnitDescriptionError, UnitKind, UnitName};
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

// #[cfg(unix)]
// const LINUX_JIG: &str = r##"
// [Jig]
// Name=Linux Jig
// Description=Development Jig running on Linux
// TestFile=/etc
// DefaultScenario=linux-tests
// DefaultWorkingDirectory=lintests
// "##;

const GENERIC_JIG: &str = r##"
[Jig]
Name=Generic Jig
Description=Generic, all-purpose jig
"##;

const THREE_TEST_SCENARIO: &str = r##"
[Scenario]
Name=Simple Scenario
Description=Just run three tests
Tests=test1, test2, test3
Timeout=200
"##;

#[cfg(windows)]
fn make_sleep_test(start: &str, delay: Option<f32>, stop: &str, ret: Option<u32>) -> String {
    let retcode = if let Some(r) = ret { r } else { 0 };

    let cmd = if let Some(d) = delay {
        format!(
            "Powershell \"Write-Output {}; start-sleep {}; Write-Output {}; exit {}\"",
            start, d, stop, retcode
        )
    } else {
        format!(
            "Powershell \"Write-Output {}; Write-Output {}; exit {}\"",
            start, stop, retcode
        )
    };
    format!(
        r##"[Test]
Name=Sleep and exit
Description=Write something, sleep for a bit, then exit
ExecStart={}
"##,
        cmd
    )
}

impl Exclave {
    pub fn new(timeout: Option<Duration>) -> Exclave {
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

    pub fn add_unit(&self, name: UnitName, unit_text: &str) {
        match *name.kind() {
            UnitKind::Test => {
                let desc =
                    TestDescription::from_string(unit_text, name, &PathBuf::from("test/config"))
                        .unwrap();
                self.manager.load_test(&desc).unwrap();
            }
            UnitKind::Jig => {
                let desc =
                    JigDescription::from_string(unit_text, name, &PathBuf::from("test/config"))
                        .unwrap();
                self.manager.load_jig(&desc).unwrap();
            }
            UnitKind::Scenario => {
                let desc = ScenarioDescription::from_string(
                    unit_text,
                    name,
                    &PathBuf::from("test/config"),
                ).unwrap();
                self.manager.load_scenario(&desc).unwrap();
            }
            _ => unimplemented!(),
        };
    }

    pub fn activate(&self, name: UnitName) {
        self.manager.activate(&name);
    }

    pub fn deactivate(&self, name: UnitName) {
        self.manager
            .deactivate(&name, "test harness requested stop");
    }
}

#[test]
/// Ensure that loading works (as a normal sanity test)
fn load_dependency() {
    let exclave = Exclave::new(None);
    exclave.add_unit(UnitName::from_str("generic", "jig").unwrap(), GENERIC_JIG);

    assert!(
        exclave
            .manager
            .jig_is_loaded(&UnitName::from_str("generic", "jig").unwrap())
    );
}

#[test]
fn basic_scenario() {
    let exclave = Exclave::new(None);

    for n in 1..=3 {
        exclave.add_unit(
            UnitName::from_str(&format!("test{}", n), "test").unwrap(),
            &make_sleep_test(
                &format!("test{}-start", n),
                None,
                &format!("test{}-end", n),
                None,
            ),
        );
    }
    exclave.add_unit(
        UnitName::from_str("three", "scenario").unwrap(),
        THREE_TEST_SCENARIO,
    );
    exclave.activate(UnitName::from_str("three", "scenario").unwrap());
}
