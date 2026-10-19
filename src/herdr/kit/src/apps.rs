//! Recognising the program running in a pane.
//!
//! The foreground application determines whether navigation keys should be
//! forwarded to the application or handled by Herdr.
//!
//! Pair these with [`crate::api::ProcessInfo::find`], which strips any
//! leading directory from the command name.

/// Whether a terminal application accepts Vim's directional key protocol.
///
/// This includes the invocation names that select a Vim mode (`view`, `ex`,
/// `vimdiff`, and the restricted/easy variants), Neovim's two invocation
/// names, and Debian's versioned Vim alternatives (`vim.basic`, `vim.tiny`).
pub fn accepts_vim_navigation(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    let command = lower.strip_suffix(".exe").unwrap_or(&lower);
    matches!(
        command,
        "vi" | "vim"
            | "vimdiff"
            | "view"
            | "ex"
            | "rvim"
            | "rview"
            | "evim"
            | "eview"
            | "nvi"
            | "nvim"
            | "nvimdiff"
    ) || command.strip_prefix("vim.").is_some_and(|variant| {
        !variant.is_empty()
            && variant
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Whether `command` is fzf itself rather than a wrapper such as `fzf-tmux`.
pub fn is_fzf(command: &str) -> bool {
    command.eq_ignore_ascii_case("fzf") || command.eq_ignore_ascii_case("fzf.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vim_navigation_variants_are_recognised() {
        for name in [
            "vim",
            "vi",
            "nvim",
            "NVIM",
            "vimdiff",
            "nvimdiff",
            "nvi",
            "view",
            "ex",
            "rvim",
            "rview",
            "evim",
            "eview",
            "vim.basic",
            "vim.tiny",
            "vim.exe",
            "nvimdiff.exe",
        ] {
            assert!(
                accepts_vim_navigation(name),
                "{name} should accept Vim navigation"
            );
        }
    }

    #[test]
    fn programs_that_merely_look_like_vim_are_not() {
        for name in ["vime", "novim", "vim.", "gvim", "vimtutor", "nvimpager", ""] {
            assert!(
                !accepts_vim_navigation(name),
                "{name} should not accept Vim navigation"
            );
        }
    }

    #[test]
    fn fzf_is_recognised_and_is_not_a_vim_navigator() {
        assert!(is_fzf("fzf"));
        assert!(is_fzf("FZF.EXE"));
        assert!(!accepts_vim_navigation("fzf"));
        assert!(!is_fzf("fzf-tmux"));
    }
}
