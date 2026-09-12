use gh_wanted::sync::{run_bounded, REFRESH_WORKERS};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Barrier, Mutex,
};

#[test]
fn workers_are_bounded_and_each_job_is_delivered_once() {
    let active = AtomicUsize::new(0);
    let maximum = AtomicUsize::new(0);
    let barrier = Barrier::new(REFRESH_WORKERS);
    let delivered = Mutex::new(Vec::new());
    run_bounded(
        (0..12).collect(),
        &AtomicBool::new(false),
        |job| {
            let count = active.fetch_add(1, Ordering::SeqCst) + 1;
            maximum.fetch_max(count, Ordering::SeqCst);
            if *job < REFRESH_WORKERS {
                barrier.wait();
            }
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(*job)
        },
        |job, result| {
            assert_eq!(job, result.unwrap());
            delivered.lock().unwrap().push(job);
            true
        },
    );
    assert_eq!(maximum.load(Ordering::SeqCst), REFRESH_WORKERS);
    let mut jobs = delivered.into_inner().unwrap();
    jobs.sort();
    assert_eq!(jobs, (0..12).collect::<Vec<_>>());
}

#[test]
fn completed_jobs_are_emitted_while_a_slow_job_waits() {
    let (sender, receiver) = std::sync::mpsc::channel();
    let receiver = Mutex::new(receiver);
    let delivered = Mutex::new(Vec::new());
    run_bounded(
        vec![0, 1],
        &AtomicBool::new(false),
        |job| {
            if *job == 0 {
                receiver
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap();
            }
            Ok(())
        },
        |job, result| {
            result.unwrap();
            delivered.lock().unwrap().push(job);
            if job == 1 {
                sender.send(()).unwrap();
            }
            true
        },
    );
    assert_eq!(delivered.into_inner().unwrap(), vec![1, 0]);
}

#[test]
fn cancelled_queue_does_not_start_jobs() {
    run_bounded(
        vec![1, 2, 3],
        &AtomicBool::new(true),
        |_| -> anyhow::Result<()> { panic!("started after cancellation") },
        |_, _| true,
    );
}
