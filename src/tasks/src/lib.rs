//! Shared, claimable work items for one project, safe across concurrent sessions.
//!
//! The store records intent and coordination: what remains, who holds a slice,
//! and the results and external references it records. Git and GitHub remain the truth about code.
//! Callers resolve the project identity and storage path; readiness is derived from
//! status, dependencies and claims rather than stored.

#[cfg(test)]
fn test_pause(root: &std::path::Path, phase: &str) {
    if std::env::var("DOTS_OUTPUT_STOP_AT").ok().as_deref() == Some(phase) {
        std::fs::write(root.join("phase-reached"), phase).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(60));
        panic!("acceptance parent did not kill paused writer");
    }
}

#[cfg(test)]
fn test_store(store: Store) -> Store {
    store
        .connection
        .execute_batch(
            "INSERT INTO projects(id,name) VALUES
         ('p-00000000000000000000000000000001','fixture'),
         ('p-00000000000000000000000000000002','other');",
        )
        .unwrap();
    store
}

mod follow;
mod goals;
pub mod outcomes;
pub mod output_discovery;
pub mod output_retention;
pub mod outputs;
mod projects;
mod references;
pub use projects::{RegisteredProject, validate_project_name};
mod store;

pub use follow::{FOLLOW_INTERVAL, Follower};
pub use goals::{Goal, GoalOverview, GoalProgress};
pub use references::ExternalReference;
pub use store::{
    Actor, Error, Event, EventKind, Filter, NewTask, Owner, Prerequisite, Project, STORE_PATH,
    Status, Store, Task, Update,
};
