//! Mac-initiated native Git and one-shot control over an explicitly selected Tailscale CLI.
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use dots_documents::mutation_lock::MutationLock;
use dots_documents::staged_directory::StagedDirectory;
use dots_process::Runner;
use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::{
    Candidate, Delivery, DeliveryReceipt, Error, Export, Identity, MAX_METADATA_BYTES, PeerRequest,
    PeerResponse, PeerStatus, Result, Role, Snapshot, Store,
};

const DEADLINE: Duration = Duration::from_secs(120);
const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_TRANSFER_DISK_BYTES: u64 = 320 * 1024 * 1024;

/// Machine-local connection data; it never belongs to the replicated knowledge tree.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub host: String,
    pub root: PathBuf,
    pub executable: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Peer {
    pub endpoint: Endpoint,
    pub identity: Identity,
}

/// Use the managed client explicitly; never auto-select another Tailscale node.
#[derive(Debug)]
pub struct Tailscale {
    executable: PathBuf,
    home: PathBuf,
}

impl Tailscale {
    pub fn new(executable: PathBuf, home: PathBuf) -> Result<Self> {
        if !executable.is_absolute()
            || !executable.is_file()
            || !home.is_absolute()
            || !home.is_dir()
        {
            return Err(crate::invalid(
                "Tailscale requires an existing absolute executable and user home",
            ));
        }
        text_path(&executable)?;
        Ok(Self { executable, home })
    }

    fn environment(&self, command: &mut Command) -> Result<()> {
        command
            .env_clear()
            .env(
                "PATH",
                env::var_os("PATH").ok_or_else(|| crate::invalid("PATH is unavailable"))?,
            )
            .env("HOME", &self.home)
            .env("TERM", "dumb")
            .env("SSH_ASKPASS_REQUIRE", "never");
        if let Some(config) =
            env::var_os("XDG_CONFIG_HOME").filter(|path| Path::new(path).is_absolute())
        {
            command.env("XDG_CONFIG_HOME", config);
        }
        Ok(())
    }

    fn request(&self, endpoint: &Endpoint, request: &PeerRequest) -> Result<PeerResponse> {
        endpoint.validate()?;
        let input = serde_json::to_vec(request).map_err(|source| Error::Json {
            operation: "peer request",
            source,
        })?;
        let remote = format!("{} memory serve", quote(text_path(&endpoint.executable)?));
        let mut command = Command::new(&self.executable);
        self.environment(&mut command)?;
        command.args(["ssh", &endpoint.host, &remote]);
        let output = Runner::new(DEADLINE, MAX_METADATA_BYTES)
            .stdin(input, MAX_REQUEST_BYTES)
            .output(command)
            .map_err(|source| Error::Process {
                operation: "exchange peer control",
                source,
            })?;
        if !output.status.success() {
            return Err(Error::Peer {
                operation: "control request",
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().into(),
            });
        }
        let response = serde_json::from_slice(&output.stdout).map_err(|source| Error::Json {
            operation: "peer response",
            source,
        })?;
        match response {
            PeerResponse::Conflict { expected, actual } => {
                Err(Error::Conflict { expected, actual })
            }
            other => Ok(other),
        }
    }

    fn git_command(&self, git: &Git, args: &[&str]) -> Result<Command> {
        let ssh = format!(
            "core.sshCommand={} ssh",
            quote(text_path(&self.executable)?)
        );
        let mut prefix = vec![
            "-c",
            "ssh.variant=simple",
            "-c",
            &ssh,
            "-c",
            "pack.allowPackReuse=false",
            "-c",
            "fetch.unpackLimit=0",
        ];
        prefix.extend_from_slice(args);
        let mut command = git.command(&prefix)?;
        // Tailscale stores generated host keys in the caller's user-config directory.
        command
            .env("HOME", &self.home)
            .env("GIT_ALLOW_PROTOCOL", "ssh")
            .env("TERM", "dumb")
            .env("SSH_ASKPASS_REQUIRE", "never");
        if let Some(config) =
            env::var_os("XDG_CONFIG_HOME").filter(|path| Path::new(path).is_absolute())
        {
            command.env("XDG_CONFIG_HOME", config);
        }
        Ok(command)
    }
}

impl Endpoint {
    fn validate(&self) -> Result<()> {
        let (user, host) = self
            .host
            .split_once('@')
            .map_or((None, self.host.as_str()), |(user, host)| {
                (Some(user), host)
            });
        if self.host.len() > 320
            || user.is_some_and(|user| {
                user.is_empty()
                    || user.starts_with('-')
                    || !user
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
            })
            || host.is_empty()
            || host.trim_end_matches('.').split('.').any(|label| {
                label.is_empty()
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
        {
            return Err(crate::invalid(
                "peer host must be a Tailscale DNS name or IPv4 address, optionally preceded by a simple user@",
            ));
        }
        for path in [&self.root, &self.executable] {
            if !path.is_absolute() {
                return Err(crate::invalid("peer root and executable must be absolute"));
            }
            text_path(path)?;
        }
        Ok(())
    }

    fn repository(&self) -> Result<String> {
        Ok(format!(
            "{}:{}",
            self.host,
            text_path(&self.root.join("repo.git"))?
        ))
    }
}

impl Peer {
    /// Discover only through the selected Tailscale route; the caller explicitly retains the binding.
    pub fn discover(local: &Store, tailscale: &Tailscale, mut endpoint: Endpoint) -> Result<Self> {
        if local.role() != Role::Authority {
            return Err(crate::invalid("only the authority initiates peer exchange"));
        }
        let response = tailscale.request(
            &endpoint,
            &PeerRequest::Status {
                root: endpoint.root.clone(),
            },
        )?;
        let PeerResponse::Status { status } = response else {
            return Err(crate::invalid("peer returned a non-status response"));
        };
        endpoint.root = status.root;
        let peer = Self {
            endpoint,
            identity: status.identity,
        };
        peer.validate(local)?;
        Ok(peer)
    }

    fn validate(&self, local: &Store) -> Result<()> {
        self.endpoint.validate()?;
        let identity = local.identity();
        if identity.role != Role::Authority
            || self.identity.role != Role::Replica
            || self.identity.node == identity.node
            || self.identity.store != identity.store
            || self.identity.genesis != identity.genesis
        {
            return Err(crate::invalid(
                "peer must be a distinct replica of this authority's knowledge store",
            ));
        }
        Ok(())
    }

    pub fn last_observed(&self, local: &Store) -> Result<Option<Snapshot>> {
        self.validate(local)?;
        local
            .git()
            .reference(&format!("refs/dots/observed/{}", self.identity.node))
    }

    pub fn last_delivery(&self, local: &Store) -> Result<Option<Snapshot>> {
        self.validate(local)?;
        local
            .git()
            .reference(&format!("refs/dots/delivered/{}", self.identity.node))
    }

    pub fn status(&self, local: &Store, tailscale: &Tailscale) -> Result<PeerStatus> {
        self.validate(local)?;
        let response = tailscale.request(
            &self.endpoint,
            &PeerRequest::Status {
                root: self.endpoint.root.clone(),
            },
        )?;
        let PeerResponse::Status { status } = response else {
            return Err(crate::invalid("peer returned a non-status response"));
        };
        if status.identity != self.identity || status.root != self.endpoint.root {
            return Err(crate::invalid(
                "peer identity or canonical storage location changed; rebind explicitly",
            ));
        }
        Ok(status)
    }

    pub fn fetch(&self, local: &Store, tailscale: &Tailscale) -> Result<Candidate> {
        let status = self.status(local, tailscale)?;
        let response = tailscale.request(
            &self.endpoint,
            &PeerRequest::Export {
                root: self.endpoint.root.clone(),
                recipient: self.identity.clone(),
                snapshot: status.snapshot.clone(),
            },
        )?;
        let PeerResponse::Export { export } = response else {
            return Err(crate::invalid("peer returned a non-export response"));
        };
        self.validate_export(&export, &status.snapshot)?;
        let observed = format!("refs/dots/observed/{}", self.identity.node);
        let previous = local.git().reference(&observed)?;
        let stage = StagedDirectory::create(local.root())?;
        let git = Git::new(stage.path().to_owned(), "quarantine".into());
        git.initialize()?;
        let mut command = tailscale.git_command(
            &git,
            &[
                "fetch",
                "--quiet",
                "--no-tags",
                "--no-auto-maintenance",
                "--no-write-fetch-head",
                "--refmap=",
                "--",
            ],
        )?;
        command.arg(self.endpoint.repository()?).arg(format!(
            "{}:{}",
            export.reference,
            crate::git::HEAD
        ));
        Git::checked(
            Git::execute(
                command,
                "fetch peer snapshot",
                Runner::new(DEADLINE, MAX_METADATA_BYTES),
            )?,
            "fetch peer snapshot",
        )?;
        bound_disk(&git.repository().join("objects"))?;
        let ancestry = crate::history::accepted(&git, &export.snapshot)?;
        if previous
            .as_ref()
            .is_some_and(|previous| !ancestry.contains(previous))
        {
            return Err(crate::invalid(
                "peer rewound or changed knowledge lineage; refusing adoption",
            ));
        }
        let known = crate::history::accepted(local.git(), &local.snapshot()?)?;
        let candidate = Candidate::retain(local, &self.identity, &export.snapshot, &git, &known)?;
        self.retain_observation(local, &candidate.snapshot)?;
        Ok(candidate)
    }

    fn validate_export(&self, export: &Export, snapshot: &Snapshot) -> Result<()> {
        if export.source != self.identity
            || export.snapshot != *snapshot
            || export.reference != format!("refs/dots/export/{snapshot}")
            || export.repository != self.endpoint.root.join("repo.git")
        {
            return Err(crate::invalid(
                "peer exported a different identity, snapshot or repository",
            ));
        }
        Ok(())
    }

    /// Return an accepted continuation; the remote owner performs the publication fence.
    pub fn deliver(
        &self,
        local: &Store,
        tailscale: &Tailscale,
        expected: &Snapshot,
        snapshot: &Snapshot,
    ) -> Result<DeliveryReceipt> {
        self.validate(local)?;
        if !local.git().is_ancestor(expected, snapshot)? {
            return Err(crate::invalid(
                "delivery would discard the observed peer history",
            ));
        }
        let export = local.prepare_export(snapshot)?;
        let candidate = Candidate {
            source: local.identity(),
            snapshot: snapshot.clone(),
        };
        let delivery = Delivery {
            recipient: self.identity.clone(),
            expected: expected.clone(),
            candidate,
        };
        let request = PeerRequest::Accept {
            root: self.endpoint.root.clone(),
            delivery,
        };
        let first = tailscale.request(&self.endpoint, &request)?;
        if let PeerResponse::Accepted { receipt } = first {
            return self.validate_receipt(local, receipt, snapshot);
        }
        let PeerResponse::NeedsTransfer {
            recipient,
            expected: observed,
            reference,
        } = first
        else {
            return Err(crate::invalid("peer returned a non-delivery response"));
        };
        let PeerRequest::Accept { delivery, .. } = &request else {
            unreachable!("request is a delivery");
        };
        if recipient != self.identity
            || observed != *expected
            || reference != delivery.candidate.transfer_reference(&self.identity)
        {
            return Err(crate::invalid(
                "peer requested a different transfer destination",
            ));
        }
        let mut command = tailscale.git_command(
            local.git(),
            &[
                "push",
                "--porcelain",
                "--receive-pack=git -c receive.hideRefs=!refs/dots/transfer/ receive-pack",
                "--",
            ],
        )?;
        command
            .arg(self.endpoint.repository()?)
            .arg(format!("{}:{reference}", export.reference));
        Git::checked(
            Git::execute(
                command,
                "send authority continuation",
                Runner::new(DEADLINE, MAX_METADATA_BYTES),
            )?,
            "send authority continuation",
        )?;
        let PeerResponse::Accepted { receipt } = tailscale.request(&self.endpoint, &request)?
        else {
            return Err(crate::invalid(
                "peer did not acknowledge the authority continuation",
            ));
        };
        self.validate_receipt(local, receipt, snapshot)
    }

    fn validate_receipt(
        &self,
        local: &Store,
        receipt: DeliveryReceipt,
        snapshot: &Snapshot,
    ) -> Result<DeliveryReceipt> {
        if receipt.recipient != self.identity || receipt.snapshot != *snapshot {
            return Err(crate::invalid(
                "peer acknowledged a different recipient or snapshot",
            ));
        }
        {
            let _lock = MutationLock::acquire(&local.root().join("mutation.lock"))?;
            advance(
                local.git(),
                &format!("refs/dots/delivered/{}", self.identity.node),
                snapshot,
            )?;
            advance(
                local.git(),
                &format!("refs/dots/observed/{}", self.identity.node),
                snapshot,
            )?;
        }
        Ok(receipt)
    }

    // Only locally retained history establishes durable replication progress.
    fn retain_observation(&self, local: &Store, snapshot: &Snapshot) -> Result<()> {
        let reference = format!("refs/dots/observed/{}", self.identity.node);
        let _lock = MutationLock::acquire(&local.root().join("mutation.lock"))?;
        advance(local.git(), &reference, snapshot)?;
        Ok(())
    }
}

fn advance(git: &Git, reference: &str, snapshot: &Snapshot) -> Result<()> {
    match git.reference(reference)? {
        Some(current) if git.is_ancestor(&current, snapshot)? => {
            git.text(
                &[
                    "update-ref",
                    "--no-deref",
                    reference,
                    snapshot.as_str(),
                    current.as_str(),
                ],
                None,
            )?;
        }
        Some(current) if git.is_ancestor(snapshot, &current)? => {}
        Some(_) => {
            return Err(crate::invalid(
                "peer observation diverged from retained knowledge",
            ));
        }
        None => git.pin(reference, snapshot)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests;

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn text_path(path: &Path) -> Result<&str> {
    let path = path
        .to_str()
        .ok_or_else(|| crate::invalid("peer paths must be Unicode"))?;
    if path.chars().any(char::is_control) {
        return Err(crate::invalid(
            "peer paths must not contain control characters",
        ));
    }
    Ok(path)
}

fn bound_disk(root: &Path) -> Result<()> {
    let mut total = 0_u64;
    for entry in dots_documents::inventory::scan(root)? {
        if entry.kind != dots_documents::inventory::Kind::File {
            return Err(crate::invalid(
                "peer object storage contains filesystem indirection",
            ));
        }
        total = total
            .checked_add(
                entry
                    .path
                    .metadata()
                    .map_err(|error| {
                        crate::io("inspect transferred object storage", &entry.path, error)
                    })?
                    .len(),
            )
            .ok_or_else(|| crate::invalid("transferred object size overflow"))?;
        if total > MAX_TRANSFER_DISK_BYTES {
            return Err(crate::invalid(
                "transferred object storage exceeds its byte budget",
            ));
        }
    }
    Ok(())
}
