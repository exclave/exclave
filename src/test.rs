use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use config::Config;

use unit::{UnitKind, UnitName};
use unitbroadcaster::{UnitBroadcaster, UnitEvent};
use unitlibrary::UnitLibrary;
use unitmanager::{ManagerControlMessage, ManagerControlMessageContents};

use units::interface::InterfaceDescription;
use units::jig::JigDescription;
use units::logger::LoggerDescription;
use units::scenario::ScenarioDescription;
use units::test::TestDescription;
use units::trigger::TriggerDescription;

struct Exclave {
    broadcaster: UnitBroadcaster,
    receiver: Receiver<UnitEvent>,
    control: Sender<ManagerControlMessage>,
    library: UnitLibrary,
}

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
fn oneliner_write_sleep_write_exit(
    start: &str,
    delay: Option<f32>,
    stop: &str,
    ret: Option<u32>,
) -> String {
    let retcode = if let Some(r) = ret { r } else { 0 };

    if let Some(d) = delay {
        format!(
            "Powershell -NoProfile -NonInteractive \"Write-Output {}; Start-Sleep {}; Write-Output {}; exit {}\"",
            start, d, stop, retcode
        )
    } else {
        format!(
            "Powershell -NoProfile -NonInteractive \"Write-Output {}; Write-Output {}; exit {}\"",
            start, stop, retcode
        )
    }
}

#[cfg(unix)]
fn oneliner_write_sleep_write_exit(
    start: &str,
    delay: Option<f32>,
    stop: &str,
    ret: Option<u32>,
) -> String {
    let retcode = if let Some(r) = ret { r } else { 0 };

    if let Some(d) = delay {
        format!(
            "/bin/sh -c \"echo '{}'; sleep {}; echo '{}'; exit {}\"",
            start, d, stop, retcode
        )
    } else {
        format!(
            "/bin/sh -c \"echo '{}'; echo '{}'; exit {}\"",
            start, stop, retcode
        )
    }
}

fn make_sleep_test(start: &str, delay: Option<f32>, stop: &str, ret: Option<u32>) -> String {
    let cmd = oneliner_write_sleep_write_exit(start, delay, stop, ret);
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
        let library = UnitLibrary::new(&broadcaster, &config);
        let control = library.get_manager().borrow().get_control_channel();
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
            broadcaster: broadcaster,
            library: library,
            receiver: receiver,
            control: control,
        }
    }

    pub fn add_unit(&self, name: &UnitName, unit_text: &str) {
        let name = name.clone();
        match *name.kind() {
            UnitKind::Test => {
                let desc =
                    TestDescription::from_string(unit_text, name, &PathBuf::from("test/config"))
                        .unwrap();
                self.library
                    .get_manager()
                    .borrow()
                    .load_test(&desc)
                    .unwrap();
            }
            UnitKind::Jig => {
                let desc =
                    JigDescription::from_string(unit_text, name, &PathBuf::from("test/config"))
                        .unwrap();
                self.library.get_manager().borrow().load_jig(&desc).unwrap();
            }
            UnitKind::Scenario => {
                let desc = ScenarioDescription::from_string(
                    unit_text,
                    name,
                    &PathBuf::from("test/config"),
                ).unwrap();
                self.library
                    .get_manager()
                    .borrow()
                    .load_scenario(&desc)
                    .unwrap();
            }
            _ => unimplemented!(),
        };
    }

    pub fn rescan(&self) {
        self.broadcaster.broadcast(&UnitEvent::RescanRequest);
    }

    // pub fn activate(&self, name: &UnitName) {
    //     self.manager.activate(name);
    // }

    // pub fn deactivate(&self, name: &UnitName) {
    //     self.manager
    //         .deactivate(name, "test harness requested stop");
    // }

    pub fn start_scenario(&self, name: &UnitName) {
        let mcmc = ManagerControlMessageContents::StartScenario(Some(name.clone()));
        self.control
            .send(ManagerControlMessage::new(name, mcmc))
            .expect("interface couldn't send exit message to controller");
    }

    pub fn run_once(&self) -> Result<UnitEvent, RecvError> {
        let msg = self.receiver.recv()?;
        self.library.process_message(&msg);
        Ok(msg)
    }

    pub fn wait_for_deactivate(&self, name: &UnitName) -> Result<(), RecvError> {
        loop {
            let msg = self.run_once()?;
            println!("Message: {:?}", msg);
            match msg {
                UnitEvent::ManagerRequest(ref mrq) => {
                    let ManagerControlMessage {
                        sender: ref sender_name,
                        contents: ref msg,
                    } = mrq;
                    match msg {
                        &ManagerControlMessageContents::ScenarioFinished(code, ref string) => {
                            println!("Got a Scenario Finished @ {}: {}", code, string);
                            assert!(sender_name == name);
                            return Ok(());
                        }
                        _ => (),
                    }
                }
                _ => (),
            }
        }
    }
}

#[test]
/// Ensure that loading works (as a normal sanity test)
fn load_dependency() {
    let exclave = Exclave::new(None);
    exclave.add_unit(&UnitName::from_str("generic", "jig").unwrap(), GENERIC_JIG);
    exclave.rescan();

    assert!(
        exclave
            .library
            .get_manager()
            .borrow()
            .jig_is_loaded(&UnitName::from_str("generic", "jig").unwrap())
    );
}

#[test]
fn basic_scenario() {
    let exclave = Exclave::new(None);
    let three_name = UnitName::from_str("three", "scenario").unwrap();

    for n in 1..=3 {
        exclave.add_unit(
            &UnitName::from_str(&format!("test{}", n), "test").unwrap(),
            &make_sleep_test(
                &format!("test{}-start", n),
                None,
                &format!("test{}-end", n),
                None,
            ),
        );
    }
    exclave.add_unit(&three_name, THREE_TEST_SCENARIO);
    exclave.rescan();

    exclave.start_scenario(&three_name);
    exclave.wait_for_deactivate(&three_name).unwrap();
}

#[test]
fn scenario_execstop() {
    let exclave = Exclave::new(None);
    let exec_stop = UnitName::from_str("execstop", "scenario").unwrap();

    exclave.add_unit(
        &UnitName::from_str("simpletest", "test").unwrap(),
        &make_sleep_test("begin", None, "end", None),
    );

    exclave.add_unit(
        &exec_stop,
        &format!(
            r##"[Scenario]
Name=Exec Stop Test
Description=Run something on stop
Tests=simpletest
ExecStop={}
"##,
            oneliner_write_sleep_write_exit("cmd-starting", Some(1.0), "cmd-ending", None)
        ),
    );
    exclave.rescan();

    exclave.start_scenario(&exec_stop);

    // Start running the main loop.  Look for the ExecStop string 'cmd is running'
    loop {
        let msg = exclave.run_once().unwrap();
        println!("Message: {:?}", msg);
        match msg {
            UnitEvent::ManagerRequest(ref mrq) => {
                let ManagerControlMessage {
                    sender: ref sender_name,
                    contents: ref msg,
                } = mrq;
                match msg {
                    &ManagerControlMessageContents::Log(ref string) => {
                        if *sender_name == exec_stop && string == "cmd-ending" {
                            return;
                        }
                    }
                    _ => (),
                }
            }
            _ => (),
        }
    }
}

#[test]
fn scenario_execstopsuccess() {
    let exclave = Exclave::new(None);
    let exec_stop = UnitName::from_str("execstopsuccess", "scenario").unwrap();

    exclave.add_unit(
        &UnitName::from_str("simpletest", "test").unwrap(),
        &make_sleep_test("begin", None, "end", None),
    );

    exclave.add_unit(
        &exec_stop,
        &format!(
            r##"[Scenario]
Name=Exec Stop Test
Description=Run something on stop
Tests=simpletest
ExecStopSuccess={}
ExecStopFailure={}
"##,
            oneliner_write_sleep_write_exit(
                "cmd-starting-success",
                Some(1.0),
                "cmd-ending-success",
                None
            ),
            oneliner_write_sleep_write_exit(
                "cmd-starting-failure",
                Some(1.0),
                "cmd-ending-failure",
                None
            )
        ),
    );
    exclave.rescan();

    exclave.start_scenario(&exec_stop);

    // Start running the main loop.  Look for the ExecStop string 'cmd is running'
    loop {
        let msg = exclave.run_once().unwrap();
        println!("Message: {:?}", msg);
        match msg {
            UnitEvent::ManagerRequest(ref mrq) => {
                let ManagerControlMessage {
                    sender: ref sender_name,
                    contents: ref msg,
                } = mrq;
                match msg {
                    &ManagerControlMessageContents::Log(ref string) => {
                        if *sender_name == exec_stop && string == "cmd-ending-success" {
                            return;
                        }
                    }
                    _ => (),
                }
            }
            _ => (),
        }
    }
}

#[test]
fn scenario_execstopfailure() {
    let exclave = Exclave::new(None);
    let exec_stop = UnitName::from_str("execstopfailure", "scenario").unwrap();

    exclave.add_unit(
        &UnitName::from_str("simpletest", "test").unwrap(),
        &make_sleep_test("begin", None, "end", Some(1)),
    );

    exclave.add_unit(
        &exec_stop,
        &format!(
            r##"[Scenario]
Name=Exec Stop Test
Description=Run something on stop
Tests=simpletest
ExecStopSuccess={}
ExecStopFailure={}
"##,
            oneliner_write_sleep_write_exit(
                "cmd-starting-success",
                Some(1.0),
                "cmd-ending-success",
                None
            ),
            oneliner_write_sleep_write_exit(
                "cmd-starting-failure",
                Some(1.0),
                "cmd-ending-failure",
                None
            )
        ),
    );
    exclave.rescan();

    exclave.start_scenario(&exec_stop);

    // Start running the main loop.  Look for the ExecStop string 'cmd is running'
    loop {
        let msg = exclave.run_once().unwrap();
        println!("Message: {:?}", msg);
        match msg {
            UnitEvent::ManagerRequest(ref mrq) => {
                let ManagerControlMessage {
                    sender: ref sender_name,
                    contents: ref msg,
                } = mrq;
                match msg {
                    &ManagerControlMessageContents::Log(ref string) => {
                        if *sender_name == exec_stop && string == "cmd-ending-failure" {
                            return;
                        }
                    }
                    _ => (),
                }
            }
            _ => (),
        }
    }
}

#[test]
/// Test that "Requires=" works.
/// Create a test "test-dependent" that
fn test_requires() {
    let exclave = Exclave::new(None);

    let scenario_name = UnitName::from_str("scenario", "scenario").unwrap();
    let master_name = UnitName::from_str("master", "test").unwrap();
    let dependent_name = UnitName::from_str("dependent", "test").unwrap();

    exclave.add_unit(
        &dependent_name,
        &make_sleep_test("begin-dependent", None, "end-dependent", None),
    );

    let mut master_test = make_sleep_test("begin-master", None, "end-master", None);
    master_test.push_str("\nRequires=dependent");
    exclave.add_unit(&master_name, &master_test);

    exclave.add_unit(
        &scenario_name,
        r##"[Scenario]
Name=Exec Stop Test
Description=Run something on stop
Tests=master
"##,
    );
    exclave.rescan();

    exclave.start_scenario(&scenario_name);
    // Ensure dependent_seen goes `true` before master_seen does.
    let mut master_seen = false;
    let mut dependent_seen = false;
    loop {
        let msg = exclave.run_once().unwrap();
        println!("Message: {:?}", msg);
        match msg {
            UnitEvent::ManagerRequest(ref mrq) => {
                let ManagerControlMessage {
                    sender: ref sender_name,
                    contents: ref msg,
                } = mrq;
                match msg {
                    &ManagerControlMessageContents::Log(ref string) => {
                        if *sender_name == dependent_name && string == "end-dependent" {
                            assert!(master_seen == false);
                            assert!(dependent_seen == false);
                            dependent_seen = true;
                        }
                        if *sender_name == master_name && string == "begin-master" {
                            assert!(master_seen == false);
                            assert!(dependent_seen == true);
                            master_seen = true;
                            return;
                        }
                    }
                    _ => (),
                }
            }
            _ => (),
        }
    }
}
