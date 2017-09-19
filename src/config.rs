use std::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 5;

macro_rules! vec_of_strings {
    ($($x:expr),*) => (vec![$($x.to_string()),*]);
}

pub struct Config {
    timeout: Duration,
    working_directory: String,
    paths: Vec<String>,
}

impl Config {
    pub fn new() -> Config {
        Config {
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            working_directory: ".".to_owned(),
            paths: vec_of_strings![
                "/usr/local/sbin",
                "/usr/local/bin",
                "/usr/sbin",
                "/usr/bin",
                "/sbin:/bin"
            ],
        }
    }

    pub fn timeout(&self) -> &Duration {
        &self.timeout
    }

    pub fn working_directory(&self) -> &String {
        &self.working_directory
    }
    pub fn paths(&self) -> &Vec<String> {
        &self.paths
    }
}