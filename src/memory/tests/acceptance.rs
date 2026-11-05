use std::fs;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::PathBuf;

use dots_memory::{Action, Change, Coordinator, Endpoint, Peer, Role, Scope, Store, Tailscale};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Lab {
    parent: PathBuf,
    authority: Store,
    replica: Store,
    tailscale: Tailscale,
    peer: Peer,
}
impl Lab {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let parent =
            std::env::temp_dir().join(format!("dots-memory-peer-{:016x}", getrandom::u64()?));
        fs::DirBuilder::new().mode(0o700).create(&parent)?;
        let authority = Store::initialize(&parent.join("authority"), "authority")?;
        let base = authority.snapshot()?;
        let bundle = parent.join("seed.bundle");
        let receipt = authority.export_bundle(&base, &bundle)?;
        let replica = Store::restore(
            &parent.join("replica"),
            "replica",
            Role::Replica,
            &bundle,
            &receipt,
        )?;
        let executable = PathBuf::from(env!("CARGO_BIN_EXE_dots-memory-fixture"));
        let tailscale = Tailscale::new(executable.clone(), parent.clone())?;
        let peer = Peer::discover(
            &authority,
            &tailscale,
            Endpoint {
                host: "replica.tail.invalid".into(),
                root: replica.root().to_owned(),
                executable,
            },
        )?;
        Ok(Self {
            parent,
            authority,
            replica,
            tailscale,
            peer,
        })
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.parent);
    }
}

fn put(path: &str, content: &str) -> Change {
    Change {
        scope: Scope::Global,
        path: path.into(),
        action: Action::Write {
            content: content.into(),
        },
    }
}

#[test]
fn native_peer_fetch_and_return_use_quarantine_and_receiver_acceptance() -> TestResult {
    let lab = Lab::new()?;
    let base = lab.authority.snapshot()?;
    let local = lab
        .authority
        .publish(&base, &[put("local.md", "local")])?
        .snapshot;
    let remote = lab
        .replica
        .publish(&base, &[put("remote.md", "remote")])?
        .snapshot;
    let candidate = lab.peer.fetch(&lab.authority, &lab.tailscale)?;
    assert_eq!(candidate.snapshot, remote);
    assert_eq!(lab.authority.snapshot()?, local);
    let coordinator = Coordinator::new(&lab.authority)?;
    let plan = coordinator.plan(&local, &candidate)?;
    let merged = coordinator.accept(&plan, &[])?;
    let receipt = lab
        .peer
        .deliver(&lab.authority, &lab.tailscale, &remote, &merged)?;
    assert_eq!(receipt.snapshot, merged);
    assert_eq!(receipt.observed, merged);
    assert_eq!(lab.replica.snapshot()?, merged);
    assert_eq!(
        lab.peer.last_delivery(&lab.authority)?,
        Some(merged.clone())
    );
    assert_eq!(
        lab.peer.last_observed(&lab.authority)?,
        Some(merged.clone())
    );
    let replay = lab
        .peer
        .deliver(&lab.authority, &lab.tailscale, &remote, &merged)?;
    assert!(replay.already_present);
    let idle = lab.peer.fetch(&lab.authority, &lab.tailscale)?;
    assert_eq!(idle.snapshot, merged);
    assert_eq!(
        coordinator.accept(&coordinator.plan(&merged, &idle)?, &[])?,
        merged
    );
    Ok(())
}

#[test]
fn peer_identity_changes_and_late_publications_fail_without_overwriting() -> TestResult {
    let lab = Lab::new()?;
    let base = lab.authority.snapshot()?;
    let local = lab
        .authority
        .publish(&base, &[put("local.md", "local")])?
        .snapshot;
    let candidate = lab.peer.fetch(&lab.authority, &lab.tailscale)?;
    let late = lab
        .replica
        .publish(&candidate.snapshot, &[put("late.md", "late")])?
        .snapshot;
    assert!(
        lab.peer
            .deliver(&lab.authority, &lab.tailscale, &base, &local)
            .is_err()
    );
    assert_eq!(lab.replica.snapshot()?, late);
    assert_eq!(lab.authority.snapshot()?, local);
    let mut wrong = lab.peer.clone();
    wrong.identity.node = lab.authority.node_id().clone();
    assert!(wrong.status(&lab.authority, &lab.tailscale).is_err());
    assert_eq!(lab.replica.snapshot()?, late);
    Ok(())
}

#[test]
fn acknowledged_history_cannot_be_silently_rewound_or_resurrected() -> TestResult {
    use std::process::Command;
    use std::time::Duration;
    let lab = Lab::new()?;
    let base = lab.authority.snapshot()?;
    let published = lab
        .authority
        .publish(&base, &[put("new.md", "new")])?
        .snapshot;
    lab.peer
        .deliver(&lab.authority, &lab.tailscale, &base, &published)?;
    let mut command = Command::new("git");
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").expect("test PATH"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .arg("--git-dir")
        .arg(lab.replica.root().join("repo.git"))
        .args([
            "update-ref",
            "--no-deref",
            "refs/heads/knowledge",
            base.as_str(),
            published.as_str(),
        ]);
    let result = dots_process::Runner::new(Duration::from_secs(10), 16 * 1024).output(command)?;
    assert!(result.status.success(), "create neutral rewind fixture");
    assert!(lab.peer.fetch(&lab.authority, &lab.tailscale).is_err());
    assert_eq!(lab.authority.snapshot()?, published);
    let mut command = Command::new("git");
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").expect("test PATH"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .arg("--git-dir")
        .arg(lab.replica.root().join("repo.git"))
        .args(["update-ref", "-d", "refs/heads/knowledge", base.as_str()]);
    let result = dots_process::Runner::new(Duration::from_secs(10), 16 * 1024).output(command)?;
    assert!(
        result.status.success(),
        "create neutral missing-head fixture"
    );
    assert!(lab.peer.status(&lab.authority, &lab.tailscale).is_err());
    assert!(Store::open(lab.replica.root()).is_err());
    assert_eq!(lab.authority.snapshot()?, published);
    Ok(())
}
