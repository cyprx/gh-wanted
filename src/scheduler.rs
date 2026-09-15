//! Cooperative request scheduling. A paused worker retains its pagination state.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Condvar, Mutex,
};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lane {
    Repositories,
    Issues,
    Activity,
}

#[derive(Default)]
struct State {
    active: Option<Lane>,
    in_flight: usize,
    failure: Option<crate::sync::RequestFailure>,
}

#[derive(Default)]
pub struct Scheduler {
    state: Mutex<State>,
    changed: Condvar,
}

impl Scheduler {
    pub fn fail(&self, policy: crate::sync::RequestFailure) {
        if !policy.paused && policy.retry_at.is_none() {
            return;
        }
        let mut state = self.state.lock().unwrap();
        let failure = state.failure.get_or_insert_with(|| policy.clone());
        failure.paused |= policy.paused;
        failure.retry_at = failure.retry_at.into_iter().chain(policy.retry_at).max();
        self.changed.notify_all();
    }

    pub fn retry(&self, now: i64) {
        let mut state = self.state.lock().unwrap();
        if state
            .failure
            .as_ref()
            .is_some_and(|f| f.retry_at.is_none_or(|at| now >= at))
        {
            state.failure = None;
        }
        self.changed.notify_all();
    }
    pub fn activate(&self, lane: Option<Lane>) {
        self.state.lock().unwrap().active = lane;
        self.changed.notify_all();
    }

    pub fn acquire(
        self: &Arc<Self>,
        lane: Lane,
        cancellation: &AtomicBool,
    ) -> anyhow::Result<Permit> {
        let mut state = self.state.lock().unwrap();
        loop {
            anyhow::ensure!(
                !cancellation.load(Ordering::Relaxed),
                "GitHub sync cancelled"
            );
            if let Some(failure) = &state.failure {
                return Err(failure.clone().into());
            }
            // Repository discovery is a prerequisite for all views.
            if (lane == Lane::Repositories || state.active == Some(lane))
                && state.in_flight < crate::sync::REFRESH_WORKERS
            {
                state.in_flight += 1;
                return Ok(Permit(self.clone()));
            }
            state = self
                .changed
                .wait_timeout(state, Duration::from_millis(50))
                .unwrap()
                .0;
        }
    }
}

pub struct Permit(Arc<Scheduler>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().in_flight -= 1;
        self.0.changed.notify_all();
    }
}
