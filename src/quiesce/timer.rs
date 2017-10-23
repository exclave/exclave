// Debounce timer taken from "notify"
// Code has license CC0-1.0 license
// https://raw.githubusercontent.com/passcod/notify/master/src/debounce/timer.rs

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use std::sync::{Arc, Condvar, Mutex};
use std::collections::{BinaryHeap, HashSet};
use std::cmp::Ordering;

use super::super::unitbroadcaster::{UnitBroadcaster, UnitEvent};

enum Action {
    Schedule(ScheduledEvent),
    Ignore(u64),
}

#[derive(PartialEq, Eq)]
struct ScheduledEvent {
    id: u64,
    when: Instant,
    event: UnitEvent,
}

impl Ord for ScheduledEvent {
    fn cmp(&self, other: &ScheduledEvent) -> Ordering {
        other.when.cmp(&self.when)
    }
}

impl PartialOrd for ScheduledEvent {
    fn partial_cmp(&self, other: &ScheduledEvent) -> Option<Ordering> {
        other.when.partial_cmp(&self.when)
    }
}

struct ScheduleWorker {
    trigger: Arc<Condvar>,
    request_source: mpsc::Receiver<Action>,
    schedule: BinaryHeap<ScheduledEvent>,
    ignore: HashSet<u64>,
    broadcaster: UnitBroadcaster,
}

impl ScheduleWorker {
    fn new(trigger: Arc<Condvar>,
           request_source: mpsc::Receiver<Action>,
           broadcaster: &UnitBroadcaster)
           -> ScheduleWorker {
        ScheduleWorker {
            trigger: trigger,
            request_source: request_source,
            schedule: BinaryHeap::new(),
            ignore: HashSet::new(),
            broadcaster: broadcaster.clone(),
        }
    }

    fn drain_request_queue(&mut self) {
        while let Ok(action) = self.request_source.try_recv() {
            match action {
                Action::Schedule(event) => self.schedule.push(event),
                Action::Ignore(ignore_id) => {
                    for &ScheduledEvent { ref id, .. } in &self.schedule {
                        if *id == ignore_id {
                            self.ignore.insert(ignore_id);
                            break;
                        }
                    }
                }
            }
        }
    }

    fn has_event_now(&self) -> bool {
        if let Some(event) = self.schedule.peek() {
            event.when <= Instant::now()
        } else {
            false
        }
    }

    fn fire_event(&mut self) {
        if let Some(ScheduledEvent { id, .. }) = self.schedule.pop() {
            if !self.ignore.remove(&id) {
                self.broadcaster.broadcast(&UnitEvent::RescanRequest);
            }
        }
    }

    fn duration_until_next_event(&self) -> Option<Duration> {
        self.schedule.peek().map(|event| {
            let now = Instant::now();
            if event.when <= now {
                Duration::from_secs(0)
            } else {
                event.when.duration_since(now)
            }
        })
    }

    fn run(&mut self) {
        let m = Mutex::new(());

        // Unwrapping is safe because the mutex can't be poisoned,
        // since we just created it.
        let mut g = m.lock().unwrap();

        loop {
            self.drain_request_queue();

            while self.has_event_now() {
                self.fire_event();
            }

            let wait_duration = self.duration_until_next_event();

            // Unwrapping is safe because the mutex can't be poisoned,
            // since we haven't shared it with another thread.
            g = if let Some(wait_duration) = wait_duration {
                self.trigger.wait_timeout(g, wait_duration).unwrap().0
            } else {
                self.trigger.wait(g).unwrap()
            };
        }
    }
}

pub struct WatchTimer {
    counter: u64,
    schedule_tx: mpsc::Sender<Action>,
    trigger: Arc<Condvar>,
    delay: Duration,
}

impl WatchTimer {
    pub fn new(broadcaster: &UnitBroadcaster,
               delay: Duration)
               -> WatchTimer {
        let (schedule_tx, schedule_rx) = mpsc::channel();
        let trigger = Arc::new(Condvar::new());

        let trigger_worker = trigger.clone();
        let mut schedule_worker = ScheduleWorker::new(trigger_worker, schedule_rx, broadcaster);
        thread::spawn(move || {
            schedule_worker.run();
        });

        WatchTimer {
            counter: 0,
            schedule_tx: schedule_tx,
            trigger: trigger,
            delay: delay,
        }
    }

    pub fn schedule(&mut self, event: UnitEvent) -> u64 {
        self.counter = self.counter.wrapping_add(1);

        self.schedule_tx
            .send(Action::Schedule(ScheduledEvent {
                id: self.counter,
                when: Instant::now() + self.delay,
                event: event,
            }))
            .expect("Failed to send a request to the global scheduling worker");

        self.trigger.notify_one();

        self.counter
    }

    pub fn ignore(&self, id: u64) {
        self.schedule_tx
            .send(Action::Ignore(id))
            .expect("Failed to send a request to the global scheduling worker");
    }
}
