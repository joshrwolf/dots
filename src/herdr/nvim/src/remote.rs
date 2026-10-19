//! Herdr owns routing; an editor owns its registration, server association and
//! process identity. Navigation is one expiring RPC, with no probing/fallback.

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail, ensure};
use herdrkit::api::Pane;
use herdrkit::{Client, PaneId, TabId};

use crate::protocol::{self, Context, Location, Operation, Reply, Request};
use crate::{Target, transport};

const TOKEN: &str = "nvim_editor";
// Leave a second of transport slack beyond the editor's mutation deadline.
const OPEN_TTL_MS: u64 = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RoutingError {
    #[error("no registered editor in this tab; use :HerdrEditorRegister")]
    NoEditor,
    #[error("multiple registered editors; invoke from the intended editor pane")]
    Ambiguous,
    #[error("editor moved or registration changed; retry from the intended tab")]
    Changed,
}

/// Connection configuration, also usable by an isolated editor owner. Tests
/// supply their own private directory; discovery never creates directories.
#[derive(Debug)]
pub struct Editors {
    executable: OsString,
    directory: PathBuf,
}

impl Editors {
    pub fn new(executable: impl Into<OsString>, directory: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            directory: directory.into(),
        }
    }

    pub fn from_env() -> Self {
        Self::new(
            std::env::var_os("HERDR_NVIM_BIN").unwrap_or_else(|| OsString::from("nvim")),
            PathBuf::from(format!("/tmp/dots-nvim-{}", nix::unistd::geteuid())),
        )
    }

    /// Select an explicit caller, otherwise require exactly one registered pane.
    /// Ambiguity is not resolved by slowly probing editors or guessing which
    /// stale marker to ignore. There is never a fallback after selecting a pane.
    pub fn open(
        &self,
        client: &Client,
        tab: &TabId,
        from: Option<&PaneId>,
        target: &Target,
    ) -> Result<PaneId> {
        ensure!(
            target.path.is_absolute() && target.line > 0 && target.column > 0,
            "file target must be absolute with one-based coordinates"
        );
        let location = Location {
            path: fs::canonicalize(&target.path).context("resolving target file")?,
            line: target.line,
            column: target.column,
        };
        let pane = select(
            client.pane_list().context("listing editor registrations")?,
            tab,
            from,
        )?;
        let registration = Registration::parse(
            pane.tokens
                .get(TOKEN)
                .context("missing editor registration")?,
        )?;
        let socket = registration.socket(&self.directory)?;
        let fresh = client
            .pane_get(&pane.pane_id)
            .context("revalidating editor pane")?;
        if fresh.pane_id != pane.pane_id
            || fresh.tab_id != *tab
            || fresh.workspace_id != pane.workspace_id
            || fresh.tokens.get(TOKEN) != Some(&registration.0)
        {
            return Err(RoutingError::Changed.into());
        }
        let context = Context {
            registration: registration.0,
            server: client.endpoint().to_owned(),
        };
        let not_after_ms =
            u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?
                .checked_add(OPEN_TTL_MS)
                .context("editor deadline overflow")?;
        let reply = transport::call(
            &self.executable,
            &socket,
            &Request {
                protocol: protocol::VERSION,
                context: context.clone(),
                operation: Operation::Open {
                    target: location.clone(),
                    not_after_ms,
                    lease: None,
                },
            },
        )?;
        match reply {
            Reply::Opened {
                identity,
                target: actual,
            } => {
                ensure!(
                    identity.protocol == protocol::VERSION
                        && identity.context == context
                        && identity.pid > 0,
                    "editor identity mismatch; inspect editor before retrying"
                );
                ensure!(
                    fs::canonicalize(&actual.path).ok().as_ref() == Some(&location.path)
                        && actual.line == location.line
                        && actual.column == location.column,
                    "editor's observed buffer/cursor do not match the target; inspect editor before retrying"
                );
            }
            Reply::Partial { error } => bail!(
                "editor partially opened target ({:?}): {}; inspect it before retrying",
                error.code,
                error.message
            ),
            Reply::Rejected { error } => bail!(
                "editor rejected target ({:?}): {}",
                error.code,
                error.message
            ),
            Reply::Ready { .. } | Reply::Ownership { .. } => {
                bail!("editor returned a control reply instead of an open acknowledgement")
            }
        }
        if from != Some(&fresh.pane_id)
            && let Err(error) = client.pane_focus(&fresh.pane_id)
        {
            eprintln!(
                "nvim: file opened in {}, but focus failed: {error:#}",
                fresh.pane_id
            );
        }
        Ok(fresh.pane_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Registration(String);

impl Registration {
    fn parse(value: &str) -> Result<Self> {
        ensure!(
            value.len() == 32
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid editor registration token"
        );
        Ok(Self(value.to_owned()))
    }

    #[expect(
        clippy::verbose_bit_mask,
        reason = "Unix permissions are clearest as an octal mask"
    )]
    fn socket(&self, directory: &Path) -> Result<PathBuf> {
        let uid = nix::unistd::geteuid().as_raw();
        let metadata =
            fs::symlink_metadata(directory).context("reading editor socket directory")?;
        ensure!(
            directory.is_absolute()
                && metadata.is_dir()
                && metadata.uid() == uid
                && metadata.mode() & 0o077 == 0,
            "editor socket directory is not a private absolute directory owned by this user"
        );
        let socket = directory.join(format!("{}.sock", self.0));
        let metadata = fs::symlink_metadata(&socket).context("reading registered editor socket")?;
        ensure!(
            metadata.file_type().is_socket() && metadata.uid() == uid,
            "registered editor endpoint is not an owned socket"
        );
        Ok(socket)
    }
}

fn select(panes: Vec<Pane>, tab: &TabId, from: Option<&PaneId>) -> Result<Pane> {
    let mut candidates: Vec<_> = panes
        .into_iter()
        .filter(|p| p.tab_id == *tab && p.tokens.contains_key(TOKEN))
        .collect();
    if let Some(index) = candidates
        .iter()
        .position(|pane| Some(&pane.pane_id) == from)
    {
        return Ok(candidates.swap_remove(index));
    }
    match candidates.len() {
        0 => Err(RoutingError::NoEditor.into()),
        1 => candidates.pop().context("registered editor disappeared"),
        _ => Err(RoutingError::Ambiguous.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pane(id: &str) -> Pane {
        Pane {
            pane_id: PaneId::new(id),
            workspace_id: herdrkit::WorkspaceId::new("w1"),
            tab_id: TabId::new("w1:t1"),
            cwd: None,
            tokens: [(
                TOKEN.to_owned(),
                "0123456789abcdef0123456789abcdef".to_owned(),
            )]
            .into(),
        }
    }
    #[test]
    fn tokens_cannot_escape_the_private_socket_directory() {
        for bad in [
            "",
            "../socket",
            "ABCDEF0123456789ABCDEF0123456789AB",
            "0123456789abcdef0123456789abcdef ",
        ] {
            assert!(Registration::parse(bad).is_err());
        }
        assert!(Registration::parse("0123456789abcdef0123456789abcdef").is_ok());
    }
    #[test]
    fn ambiguity_never_selects_an_arbitrary_editor() {
        let tab = TabId::new("w1:t1");
        assert_eq!(
            select(vec![], &tab, None).unwrap_err().downcast_ref(),
            Some(&RoutingError::NoEditor)
        );
        assert_eq!(
            select(vec![pane("w1:p1"), pane("w1:p2")], &tab, None)
                .unwrap_err()
                .downcast_ref(),
            Some(&RoutingError::Ambiguous)
        );
        let chosen = select(
            vec![pane("w1:p1"), pane("w1:p2")],
            &tab,
            Some(&PaneId::new("w1:p2")),
        )
        .unwrap();
        assert_eq!(chosen.pane_id, PaneId::new("w1:p2"));
    }
}
