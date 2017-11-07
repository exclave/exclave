use std::cell::RefCell;
use std::env;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 5;

pub struct Config {
    timeout: Duration,
    global_working_directory: PathBuf,
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
            global_working_directory: env::current_dir().expect("Couldn't get current working directory"),
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

    pub fn working_directory(&self, default: &Option<PathBuf>) -> PathBuf {
        match *default {
            Some(ref s) => s.clone(),
            None => match *self.scenario_working_directory.borrow() {
                Some(ref s) => s.clone(),
                None => match *self.jig_working_directory.borrow() {
                    Some(ref s) => s.clone(),
                    None => self.global_working_directory.clone(),
                }
            }
        }
    }

    pub fn paths(&self) -> &Vec<PathBuf> {
        &self.paths
    }

    pub fn set_jig_working_directory(&self, new_buf: &Option<PathBuf>) {
        *self.jig_working_directory.borrow_mut() = new_buf.clone();
    }

    pub fn set_scenario_working_directory(&self, new_buf: &Option<PathBuf>) {
        *self.scenario_working_directory.borrow_mut() = new_buf.clone();
    }
}
