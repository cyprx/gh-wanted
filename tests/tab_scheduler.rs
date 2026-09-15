use gh_wanted::{
    scheduler::{Lane, Scheduler},
    sync::RequestFailure,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};

#[test]
fn tab_switch_pauses_next_request_and_resumes_without_replaying() {
    let scheduler = Arc::new(Scheduler::default());
    let cancellation = Arc::new(AtomicBool::new(false));
    scheduler.activate(Some(Lane::Activity));
    let first = scheduler.acquire(Lane::Activity, &cancellation).unwrap();
    scheduler.activate(Some(Lane::Issues));
    let (sent, received) = mpsc::channel();
    let worker = {
        let scheduler = scheduler.clone();
        let cancellation = cancellation.clone();
        thread::spawn(move || {
            let _next_page = scheduler.acquire(Lane::Activity, &cancellation).unwrap();
            sent.send("page two").unwrap();
        })
    };
    // In-flight work can finish; a new tab need not wait for the old feed to finish.
    let issues = scheduler.acquire(Lane::Issues, &cancellation).unwrap();
    drop(first);
    assert!(received.recv_timeout(Duration::from_millis(100)).is_err());
    drop(issues);
    scheduler.activate(Some(Lane::Activity));
    assert_eq!(
        received.recv_timeout(Duration::from_secs(2)).unwrap(),
        "page two"
    );
    worker.join().unwrap();
}

#[test]
fn shared_limit_and_cancelled_paused_workers() {
    let scheduler = Arc::new(Scheduler::default());
    let cancellation = Arc::new(AtomicBool::new(false));
    scheduler.activate(Some(Lane::Issues));
    let mut permits: Vec<_> = (0..3)
        .map(|_| scheduler.acquire(Lane::Issues, &cancellation).unwrap())
        .collect();
    scheduler.activate(Some(Lane::Activity));
    let (sent, received) = mpsc::channel();
    let worker = {
        let scheduler = scheduler.clone();
        let cancellation = cancellation.clone();
        thread::spawn(move || {
            let _permit = scheduler.acquire(Lane::Activity, &cancellation).unwrap();
            sent.send(()).unwrap();
        })
    };
    assert!(received.recv_timeout(Duration::from_millis(100)).is_err());
    permits.pop();
    received.recv_timeout(Duration::from_secs(2)).unwrap();
    worker.join().unwrap();
    scheduler.activate(None);
    let paused = {
        let scheduler = scheduler.clone();
        let cancellation = cancellation.clone();
        thread::spawn(move || scheduler.acquire(Lane::Issues, &cancellation).is_err())
    };
    cancellation.store(true, Ordering::Relaxed);
    assert!(paused.join().unwrap());
}

#[test]
fn rate_limit_stops_every_lane_and_cannot_be_reset_early() {
    let scheduler = Arc::new(Scheduler::default());
    let cancellation = AtomicBool::new(false);
    scheduler.fail(RequestFailure {
        paused: false,
        retry_at: Some(100),
    });
    scheduler.retry(99);
    for lane in [Lane::Activity, Lane::Issues, Lane::Repositories] {
        scheduler.activate(Some(lane));
        assert!(scheduler.acquire(lane, &cancellation).is_err());
    }
    scheduler.retry(100);
    assert!(scheduler.acquire(Lane::Repositories, &cancellation).is_ok());
}
