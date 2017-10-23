// Debounce timer taken from "notify"
// Code has license CC0-1.0 license
// https://raw.githubusercontent.com/passcod/notify/master/src/debounce/timer.rs

mod timer;

use self::timer::WatchTimer;
use super::unitbroadcaster::{UnitBroadcaster, UnitEvent};

use std::time::Duration;

pub struct Quiesce {
    timer_id: Option<u64>,
    timer: WatchTimer,
}

impl Quiesce {
    pub fn new(delay: Duration, broadcaster: &UnitBroadcaster) -> Quiesce {
        Quiesce {
            timer_id: None,
            timer: WatchTimer::new(broadcaster, delay),
        }
    }
    pub fn process_message(&mut self, evt: &UnitEvent) {
        if evt == &UnitEvent::RescanRequest || evt == &UnitEvent::RescanStart || evt == &UnitEvent::RescanFinish {
            return;
        }
        self.restart_timer(UnitEvent::RescanRequest);
    }

    fn restart_timer(&mut self, event: UnitEvent) {
        if let Some(timer_id) = self.timer_id {
            self.timer.ignore(timer_id);
        }
        self.timer_id = Some(self.timer.schedule(event));
    }

}