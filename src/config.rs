use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 5;

pub struct Config {
    timeout: Duration,
    working_directory: PathBuf,
    paths: Vec<PathBuf>,
}

impl Config {
    pub fn new() -> Config {
        Config {
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            working_directory: Path::new(".").to_owned(),
            paths: vec![
                Path::new("/usr/local/sbin").to_owned(),
                Path::new("/usr/local/bin").to_owned(),
                Path::new("/usr/sbin").to_owned(),
                Path::new("/usr/bin").to_owned(),
                Path::new("/sbin:/bin").to_owned()
            ],
        }
    }

    pub fn timeout(&self) -> &Duration {
        &self.timeout
    }

    pub fn working_directory(&self) -> &PathBuf {
        &self.working_directory
    }
    pub fn paths(&self) -> &Vec<PathBuf> {
        &self.paths
    }
}