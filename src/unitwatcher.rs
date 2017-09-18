extern crate notify;

use std::path::{Path, PathBuf};
use std::io;
use std::sync::mpsc::channel;
use std::time::Duration;
use std::thread;

use unitbroadcaster::*;

use self::notify::{RecommendedWatcher, Watcher, RecursiveMode};

pub struct UnitWatcher {
    paths: Vec<PathBuf>,
    watcher: RecommendedWatcher,
    broadcaster: UnitBroadcaster,
}

impl UnitWatcher {
    pub fn new(broadcaster: &UnitBroadcaster) -> UnitWatcher {
        let (watcher_tx, watcher_rx) = channel();

        // Automatically select the best implementation for your platform.
        // You can also access each implementation directly e.g. INotifyWatcher.
        let watcher: RecommendedWatcher = Watcher::new(watcher_tx, Duration::from_secs(2))
            .expect("Unable to create file watcher");

        // This is a simple loop, but you may want to use more complex logic here,
        // for example to handle I/O.
        let thread_broadcaster = broadcaster.clone();
        thread::spawn(move || {
            loop {
                match watcher_rx.recv() {
                    Ok(event) => {
                        // Convert the DebouncedEvent into a UnitEvent
                        let status_event = match event {
                            notify::DebouncedEvent::Create(path) => {
                                UnitEvent::Status(UnitStatusEvent {
                                    name: UnitName::from_path(&path).unwrap(),
                                    status: UnitStatus::Added,
                                })
                            }
                            notify::DebouncedEvent::Write(path) => {
                                UnitEvent::Status(UnitStatusEvent {
                                    name: UnitName::from_path(&path).unwrap(),
                                    status: UnitStatus::Updated,
                                })
                            }
                            notify::DebouncedEvent::Remove(path) => {
                                UnitEvent::Status(UnitStatusEvent {
                                    name: UnitName::from_path(&path).unwrap(),
                                    status: UnitStatus::Deleted,
                                })
                            }
                            _ => continue,
                        };

                        // Send a copy of the message to each of the listeners.
                        thread_broadcaster.broadcast(&status_event);
                    }
                    Err(e) => println!("watch error: {:?}", e),
                }
            }
        });

        UnitWatcher {
            paths: vec![],
            broadcaster: broadcaster.clone(),
            watcher: watcher,
        }
    }

    fn add_unit(&mut self, unit_name: UnitName) {
        self.broadcaster.broadcast(&UnitEvent::Status(UnitStatusEvent {
                    name: unit_name.clone(),
                    status: UnitStatus::Added,
                }));
    }

    pub fn add_path(&mut self, config_dir: &str) -> Result<(), io::Error> {
        let dir = Path::new(config_dir);
        for entry in dir.read_dir()? {
            let unit_name = match UnitName::from_path(&entry?.path()) {
                None => continue,
                Some(s) => s,
            };

            self.add_unit(unit_name);
        }

        self.watch(&dir).expect("Unable to watch directory");
        self.paths.push(dir.to_owned());
        Ok(())
    }

    fn watch(&mut self, path: &Path) -> notify::Result<()> {

        // Add a path to be watched. All files and directories at that path and
        // below will be monitored for changes.
        try!(self.watcher.watch(path, RecursiveMode::Recursive));

        Ok(())
    }
}