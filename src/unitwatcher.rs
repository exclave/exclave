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
                            notify::DebouncedEvent::Create(path) => UnitStatusEvent::new_added(&path),
                            notify::DebouncedEvent::Write(path) => UnitStatusEvent::new_updated(&path),
                            notify::DebouncedEvent::Remove(path) => UnitStatusEvent::new_removed(&path),
                            _ => None,
                        };

                        // Send a copy of the message to each of the listeners.
                        if let Some(evt) = status_event {
                            thread_broadcaster.broadcast(&UnitEvent::Status(evt));
                        }
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

    pub fn add_path(&mut self, config_dir: &str) -> Result<(), io::Error> {
        let dir = Path::new(config_dir);
        for entry in dir.read_dir()? {
           if let Some(evt) = UnitStatusEvent::new_added(&entry?.path()) {
               self.broadcaster.broadcast(&UnitEvent::Status(evt));
           }
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