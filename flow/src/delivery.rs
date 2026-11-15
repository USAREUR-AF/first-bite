//! What each subscriber was owed and what it delivered.
//!
//! A publisher socket forgets a record once it is sent, and a subscriber
//! reads it some time later. At the end of a run the process has to know
//! whether every subscriber has caught up before it exits. This module holds
//! that account: the publishers add to what a subscriber is owed each time
//! they send a record on a topic it takes, and the subscriber adds to what it
//! delivered or failed to deliver. The difference is what is still on the
//! way.

use crate::feed::TopicFilter;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Records a subscriber may be behind before a file replay waits for it. The
/// sockets between publisher and subscriber hold about 2,000 messages and
/// drop what does not fit.
pub const BACKLOG_LIMIT: u64 = 500;

/// How long a file replay waits on a subscriber that delivers nothing before
/// it stops waiting for that subscriber.
const STALLED_AFTER: Duration = Duration::from_secs(5);

/// The account of one subscriber.
#[derive(Debug)]
pub(crate) struct Account {
    pub(crate) name: String,
    /// The local port the subscriber reads, when it reads this machine.
    pub(crate) local_port: Option<String>,
    pub(crate) filter: TopicFilter,
    /// Subscriptions its socket makes: one per topic, or one for everything.
    pub(crate) subscriptions: u64,
    /// Whether a publisher of this process feeds it, so `owed` is known.
    tracked: AtomicBool,
    owed: AtomicU64,
    delivered: AtomicU64,
    failed: AtomicU64,
    /// Records the sink accepted and was still holding when it finished.
    held: AtomicU64,
    /// Records the sink accepted and reported lost afterwards.
    lost: AtomicU64,
    finished: AtomicBool,
    /// A file replay no longer waits for this subscriber.
    stalled: AtomicBool,
}

impl Account {
    pub(crate) fn new(
        name: &str,
        local_port: Option<String>,
        filter: TopicFilter,
        subscriptions: u64,
    ) -> Self {
        Account {
            name: name.to_string(),
            local_port,
            filter,
            subscriptions,
            tracked: AtomicBool::new(false),
            owed: AtomicU64::new(0),
            delivered: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            held: AtomicU64::new(0),
            lost: AtomicU64::new(0),
            finished: AtomicBool::new(false),
            stalled: AtomicBool::new(false),
        }
    }

    pub(crate) fn delivered(&self) {
        self.delivered.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn failed(&self) {
        self.failed.fetch_add(1, Ordering::Relaxed);
    }

    fn processed(&self) -> u64 {
        self.delivered.load(Ordering::Relaxed) + self.failed.load(Ordering::Relaxed)
    }

    fn backlog(&self) -> u64 {
        self.owed
            .load(Ordering::Relaxed)
            .saturating_sub(self.processed())
    }

    /// Whether everything owed has been handed to the sink. A subscriber no
    /// publisher of this process feeds has no account of what it is owed.
    pub(crate) fn caught_up(&self) -> bool {
        self.tracked.load(Ordering::Relaxed) && self.backlog() == 0
    }

    pub(crate) fn is_tracked(&self) -> bool {
        self.tracked.load(Ordering::Relaxed)
    }

    /// The subscriber has stopped: `held` records were still in the sink and
    /// `lost` were accepted by it and not delivered.
    pub(crate) fn finish(&self, held: u64, lost: u64) {
        self.held.store(held, Ordering::Relaxed);
        self.lost.store(lost, Ordering::Relaxed);
        self.finished.store(true, Ordering::Relaxed);
    }

    fn report(&self) -> SinkReport {
        let ok = self.delivered.load(Ordering::Relaxed);
        let held = self.held.load(Ordering::Relaxed).min(ok);
        let lost = self.lost.load(Ordering::Relaxed).min(ok - held);
        SinkReport {
            name: self.name.clone(),
            delivered: ok - held - lost,
            failed: self.failed.load(Ordering::Relaxed) + lost,
            abandoned: self.backlog() + held,
            stopped: self.finished.load(Ordering::Relaxed),
        }
    }
}

/// What one sink did with the records of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SinkReport {
    /// The subscriber's name.
    pub name: String,
    /// Records the sink delivered.
    pub delivered: u64,
    /// Records the sink tried and could not deliver.
    pub failed: u64,
    /// Records published for the sink that it had not delivered when the
    /// time to drain ran out, or that never reached it.
    pub abandoned: u64,
    /// Whether the subscriber had stopped when the report was made; one that
    /// had not was still inside a delivery.
    pub stopped: bool,
}

impl SinkReport {
    /// Whether the run should be reported as failed for this sink: a record
    /// was abandoned, or records were owed and none was delivered.
    pub fn incomplete(&self) -> bool {
        self.abandoned > 0 || (self.delivered == 0 && self.failed > 0)
    }
}

impl std::fmt::Display for SinkReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "sink {}: {} delivered, {} failed, {} abandoned",
            self.name, self.delivered, self.failed, self.abandoned
        )?;
        if !self.stopped {
            f.write_str(" (still inside a delivery when the time to drain ran out)")?;
        }
        Ok(())
    }
}

/// The accounts of every subscriber of the process.
#[derive(Debug, Default)]
pub struct Tracker {
    accounts: Vec<Arc<Account>>,
    /// Set when input has ended: subscribers stop once they have caught up.
    draining: AtomicBool,
    /// Set when the time to drain has run out: subscribers stop now.
    expired: AtomicBool,
    /// When that will be, once draining has begun.
    deadline: std::sync::Mutex<Option<Instant>>,
}

impl Tracker {
    pub(crate) fn new(accounts: Vec<Arc<Account>>) -> Self {
        Tracker {
            accounts,
            draining: AtomicBool::new(false),
            expired: AtomicBool::new(false),
            deadline: std::sync::Mutex::new(None),
        }
    }

    /// The time left to drain; `None` before draining has begun.
    pub(crate) fn time_left(&self) -> Option<Duration> {
        let deadline = *self
            .deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    pub(crate) fn is_draining(&self) -> bool {
        self.draining.load(Ordering::Relaxed)
    }

    pub(crate) fn is_expired(&self) -> bool {
        self.expired.load(Ordering::Relaxed)
    }

    /// The subscribers that read `port` on this machine are fed by a
    /// publisher of this process. Returns the subscriptions they make.
    pub(crate) fn feed_from(&self, port: &str) -> u64 {
        let mut subscriptions = 0;
        for account in &self.accounts {
            if account.local_port.as_deref() == Some(port) {
                account.tracked.store(true, Ordering::Relaxed);
                subscriptions += account.subscriptions;
            }
        }
        subscriptions
    }

    /// A record went out on `topic` from the publisher bound to `port`.
    pub(crate) fn published(&self, port: &str, topic: &str) {
        for account in &self.accounts {
            if account.local_port.as_deref() == Some(port) && account.filter.matches(topic) {
                account.owed.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Wait while a subscriber is more than [`BACKLOG_LIMIT`] records behind.
    /// A file is read faster than a sink delivers, and the sockets between
    /// them drop what they cannot hold; a replay calls this after each record
    /// so that nothing is dropped there. A subscriber that delivers nothing
    /// for five seconds is no longer waited for.
    pub fn pace(&self) {
        for account in &self.accounts {
            if account.stalled.load(Ordering::Relaxed) || account.backlog() <= BACKLOG_LIMIT {
                continue;
            }
            let mut progress = (account.processed(), Instant::now());
            while account.backlog() > BACKLOG_LIMIT {
                std::thread::sleep(Duration::from_millis(1));
                let processed = account.processed();
                if processed != progress.0 {
                    progress = (processed, Instant::now());
                } else if progress.1.elapsed() >= STALLED_AFTER {
                    account.stalled.store(true, Ordering::Relaxed);
                    log::warn!(
                        "subscriber {}: nothing delivered for {STALLED_AFTER:?}; the replay no \
                         longer waits for it, and records beyond what the sockets hold are lost \
                         to it",
                        account.name
                    );
                    break;
                }
            }
        }
    }

    /// Input has ended. Give every subscriber up to `within` to deliver what
    /// it is owed and what its sink holds, then report. Subscribers stop as
    /// they catch up; one still inside a delivery when the time runs out is
    /// reported as it stands.
    pub fn drain(&self, within: Duration) -> Vec<SinkReport> {
        let deadline = Instant::now() + within;
        *self
            .deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(deadline);
        self.draining.store(true, Ordering::Relaxed);
        let all_finished = || {
            self.accounts
                .iter()
                .all(|account| account.finished.load(Ordering::Relaxed))
        };
        while !all_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        self.expired.store(true, Ordering::Relaxed);
        // A subscriber between deliveries notices within one poll interval.
        let grace = Instant::now() + Duration::from_millis(500);
        while !all_finished() && Instant::now() < grace {
            std::thread::sleep(Duration::from_millis(10));
        }
        self.accounts
            .iter()
            .map(|account| account.report())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(topics: &[&str], port: &str) -> Arc<Account> {
        let topics: Vec<String> = topics.iter().map(ToString::to_string).collect();
        Arc::new(Account::new(
            "A",
            Some(port.to_string()),
            TopicFilter::new(&topics),
            topics.len() as u64,
        ))
    }

    #[test]
    fn a_subscriber_is_owed_what_is_published_on_its_port_and_topics() {
        let one = account(&["WX/Position"], "12000");
        let all = account(&["*"], "12000");
        let elsewhere = account(&["*"], "13000");
        let tracker = Tracker::new(vec![one.clone(), all.clone(), elsewhere.clone()]);
        assert_eq!(tracker.feed_from("12000"), 2);
        for topic in ["WX/Position", "WX/Heartbeat", "WX/Position"] {
            tracker.published("12000", topic);
        }
        assert_eq!(one.backlog(), 2);
        assert_eq!(all.backlog(), 3);
        assert!(!elsewhere.is_tracked());
        one.delivered();
        one.failed();
        assert!(one.caught_up());
        assert!(!all.caught_up());
    }

    #[test]
    fn the_report_separates_delivered_failed_and_abandoned() {
        let account = account(&["*"], "12000");
        let tracker = Tracker::new(vec![account.clone()]);
        tracker.feed_from("12000");
        for _ in 0..10 {
            tracker.published("12000", "t");
        }
        for _ in 0..6 {
            account.delivered();
        }
        account.failed();
        // The sink still held two of the six and had lost one.
        account.finish(2, 1);
        let report = tracker.drain(Duration::from_millis(1)).remove(0);
        assert_eq!(
            (report.delivered, report.failed, report.abandoned),
            (3, 2, 5)
        );
        assert!(report.incomplete());
        assert_eq!(
            report.to_string(),
            "sink A: 3 delivered, 2 failed, 5 abandoned"
        );
    }

    #[test]
    fn a_sink_that_delivered_everything_or_was_owed_nothing_is_complete() {
        let complete = SinkReport {
            name: "A".into(),
            delivered: 5,
            failed: 0,
            abandoned: 0,
            stopped: true,
        };
        assert!(!complete.incomplete());
        let some_failed = SinkReport {
            failed: 2,
            ..complete.clone()
        };
        assert!(!some_failed.incomplete());
        let never = SinkReport {
            delivered: 0,
            failed: 5,
            ..complete.clone()
        };
        assert!(never.incomplete());
        let idle = SinkReport {
            delivered: 0,
            ..complete.clone()
        };
        assert!(!idle.incomplete());
        let unfinished = SinkReport {
            stopped: false,
            abandoned: 1,
            ..complete
        };
        assert!(unfinished.to_string().ends_with("ran out)"));
    }
}
