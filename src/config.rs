use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 5;

pub struct Config {
    timeout: Duration,
    jig_working_directory: Rc<RefCell<Option<PathBuf>>>,
    scenario_working_directory: Rc<RefCell<Option<PathBuf>>>,
    paths: Vec<PathBuf>,
    terminate_timeout: Duration,
}

impl Config {
    pub fn new() -> Config {
        Config {
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            terminate_timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            jig_working_directory: Rc::new(RefCell::new(None)),
            scenario_working_directory: Rc::new(RefCell::new(None)),
            paths: vec![
                Path::new("/usr/local/sbin").to_owned(),
                Path::new("/usr/local/bin").to_owned(),
                Path::new("/usr/sbin").to_owned(),
                Path::new("/usr/bin").to_owned(),
                Path::new("/sbin:/bin").to_owned(),
            ],
        }
    }

    pub fn timeout(&self) -> &Duration {
        &self.timeout
    }

    pub fn terminate_timeout(&self) -> &Duration {
        &self.terminate_timeout
    }

    /// Return a working directory composed of the unit's directory,
    /// the jig working directory, and the scenario working directory.
    pub fn working_directory(&self, default: &Path, wd: &Option<PathBuf>) -> PathBuf {
        // println!(">>>");
        // println!("Default directory: {:?}", default);
        // println!("Specified wd: {:?}", wd);
        // println!("Jig directory: {:?}", self.jig_working_directory.borrow());
        // println!("Scenario directory: {:?}", self.scenario_working_directory.borrow());
        // println!("<<<");
        let mut p = default.to_owned();
        if let Some(jwd) = &*self.jig_working_directory.borrow() {
            p.push(jwd);
        }
        if let Some(swd) = &*self.scenario_working_directory.borrow() {
            p.push(swd);
        }
        if let Some(wd) = wd {
            p.push(wd);
        }
        match p.canonicalize() {
            Ok(x) => x,
            Err(_) => p,
        }
    }

    pub fn paths(&self) -> &Vec<PathBuf> {
        &self.paths
    }

    pub fn set_jig_working_directory(&self, new_path: &Path) {
        *self.jig_working_directory.borrow_mut() = Some(new_path.to_owned());
    }

    pub fn clear_jig_working_directory(&self) {
        *self.jig_working_directory.borrow_mut() = None;
    }

    pub fn set_scenario_working_directory(&self, new_path: &Path) {
        *self.scenario_working_directory.borrow_mut() = Some(new_path.to_owned());
    }

    pub fn clear_scenario_working_directory(&self) {
        *self.scenario_working_directory.borrow_mut() = None;
    }
}
