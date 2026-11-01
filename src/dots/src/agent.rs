//! The coding-agent session a command runs in, from the variables each agent
//! sets for the shell commands it runs, and the Rex session its work belongs to.

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
    use super::{rex_session_from, session_from};

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
