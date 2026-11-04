//! The coding-agent session a command runs in, from the variables each agent
//! sets for the shell commands it runs, the live runtime acting for it, and
//! the Rex session its work belongs to.

/// Variables naming an agent session, with the prefix that keeps their IDs
/// apart. When several are set (an agent started inside another), the first wins.
const SESSIONS: [(&str, &str); 3] = [
    ("PI_SESSION_ID", "pi"),
    ("CLAUDE_CODE_SESSION_ID", "claude"),
    ("CODEX_SESSION_ID", "codex"),
];

/// The invoking agent session, such as `pi-<id>`, if any.
pub(crate) fn session() -> Option<String> {
    session_from(|name| std::env::var(name).ok())
}

fn session_from(get: impl Fn(&str) -> Option<String>) -> Option<String> {
    SESSIONS.iter().find_map(|(name, prefix)| {
        get(name)
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty())
            .map(|id| format!("{prefix}-{id}"))
    })
}

/// The variable a host sets to name its live runtime: `<session>-<nonce>`.
const OWNER: &str = "DOTS_AGENT_OWNER";

/// Lowercase hexadecimal digits in a runtime nonce.
const NONCE_LEN: usize = 16;

/// The actor for claims and coordination events, such as
/// `pi-<id>-<nonce>`, if any.
///
/// A host exports `$DOTS_AGENT_OWNER` to name the live runtime acting for its
/// session; two runtimes of one session get different nonces. The value counts
/// only when it names the invoking agent session, so a nested agent with its
/// own session never acts as the runtime it inherited the value from. Without
/// an agent session, as in a host's own subprocesses, it must still name one.
/// Otherwise the actor is the agent session itself. This is cooperative
/// provenance, not authentication: any process can set the variable.
pub(crate) fn owner() -> Option<String> {
    owner_from(|name| std::env::var(name).ok())
}

fn owner_from(get: impl Fn(&str) -> Option<String>) -> Option<String> {
    let session = session_from(&get);
    get(OWNER)
        .map(|owner| owner.trim().to_owned())
        .filter(|owner| names_runtime_of(owner, session.as_deref()))
        .or(session)
}

/// Whether `owner` is `<session>-<nonce>` for the invoking `session`, or for
/// some agent session when none is invoking.
fn names_runtime_of(owner: &str, session: Option<&str>) -> bool {
    let Some((named, nonce)) = owner.rsplit_once('-') else {
        return false;
    };
    if nonce.len() != NONCE_LEN
        || !nonce
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return false;
    }
    match session {
        Some(session) => named == session,
        None => SESSIONS.iter().any(|(_, prefix)| {
            named
                .strip_prefix(prefix)
                .and_then(|rest| rest.strip_prefix('-'))
                .is_some_and(|id| !id.trim().is_empty())
        }),
    }
}

/// The Rex session the invoking work belongs to, such as `session:<id>`.
///
/// A terminal in Rex sets `REX_SESSION`; a headless worker has none and gets
/// `DOTS_REX_SESSION` from the session that started it.
pub(crate) fn rex_session() -> Option<String> {
    rex_session_from(|name| std::env::var(name).ok())
}

fn rex_session_from(get: impl Fn(&str) -> Option<String>) -> Option<String> {
    ["REX_SESSION", "DOTS_REX_SESSION"].iter().find_map(|name| {
        get(name)
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::{owner_from, rex_session_from, session_from};

    const NONCE: &str = "0123456789abcdef";

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn each_agent_is_prefixed() {
        assert_eq!(
            session_from(env(&[("PI_SESSION_ID", "a")])).as_deref(),
            Some("pi-a")
        );
        assert_eq!(
            session_from(env(&[("CLAUDE_CODE_SESSION_ID", "b")])).as_deref(),
            Some("claude-b")
        );
        assert_eq!(
            session_from(env(&[("CODEX_SESSION_ID", "c")])).as_deref(),
            Some("codex-c")
        );
    }

    #[test]
    fn blank_values_are_absent_and_the_first_set_wins() {
        assert_eq!(session_from(env(&[("PI_SESSION_ID", "  ")])), None);
        assert_eq!(
            session_from(env(&[("CODEX_SESSION_ID", "c"), ("PI_SESSION_ID", "a")])).as_deref(),
            Some("pi-a")
        );
        assert_eq!(session_from(env(&[])), None);
    }

    #[test]
    fn a_runtime_of_the_invoking_session_is_the_owner() {
        let owner = format!("pi-a-{NONCE}");
        assert_eq!(
            owner_from(env(&[
                ("PI_SESSION_ID", "a"),
                ("DOTS_AGENT_OWNER", &format!(" {owner} "))
            ])),
            Some(owner)
        );
    }

    #[test]
    fn without_a_runtime_the_session_is_the_owner() {
        assert_eq!(
            owner_from(env(&[("PI_SESSION_ID", "a")])).as_deref(),
            Some("pi-a")
        );
        assert_eq!(
            owner_from(env(&[("PI_SESSION_ID", "a"), ("DOTS_AGENT_OWNER", " ")])).as_deref(),
            Some("pi-a")
        );
        assert_eq!(owner_from(env(&[])), None);
    }

    #[test]
    fn an_inherited_runtime_of_another_session_is_ignored() {
        // A nested agent resets its session; the parent's runtime is not its own.
        let parent = format!("pi-parent-{NONCE}");
        assert_eq!(
            owner_from(env(&[
                ("PI_SESSION_ID", "child"),
                ("DOTS_AGENT_OWNER", &parent)
            ]))
            .as_deref(),
            Some("pi-child")
        );
        assert_eq!(
            owner_from(env(&[
                ("CODEX_SESSION_ID", "c"),
                ("DOTS_AGENT_OWNER", &parent)
            ]))
            .as_deref(),
            Some("codex-c")
        );
        // A session ID that merely prefixes the named one is a different session.
        assert_eq!(
            owner_from(env(&[
                ("PI_SESSION_ID", "a"),
                ("DOTS_AGENT_OWNER", &format!("pi-a-b-{NONCE}"))
            ]))
            .as_deref(),
            Some("pi-a")
        );
    }

    #[test]
    fn a_host_subprocess_without_a_session_uses_a_well_formed_runtime() {
        let owner = format!("pi-a-{NONCE}");
        assert_eq!(
            owner_from(env(&[("DOTS_AGENT_OWNER", &owner)])),
            Some(owner)
        );
        for malformed in [
            "pi-a".to_owned(),
            format!("pi-{NONCE}"),
            format!("pi- -{NONCE}"),
            format!("other-a-{NONCE}"),
            format!("pi-a-{}", NONCE.to_uppercase()),
            format!("pi-a-{NONCE}0"),
            format!("pi-a-{}", &NONCE[1..]),
        ] {
            assert_eq!(
                owner_from(env(&[("DOTS_AGENT_OWNER", &malformed)])),
                None,
                "{malformed}"
            );
        }
    }

    #[test]
    fn a_terminal_rex_session_wins_over_an_inherited_one() {
        assert_eq!(
            rex_session_from(env(&[
                ("DOTS_REX_SESSION", "session:top"),
                ("REX_SESSION", "session:here"),
            ]))
            .as_deref(),
            Some("session:here")
        );
        assert_eq!(
            rex_session_from(env(&[
                ("REX_SESSION", " "),
                ("DOTS_REX_SESSION", "session:top")
            ]))
            .as_deref(),
            Some("session:top")
        );
        assert_eq!(rex_session_from(env(&[("DOTS_REX_SESSION", "")])), None);
        assert_eq!(rex_session_from(env(&[])), None);
    }
}
