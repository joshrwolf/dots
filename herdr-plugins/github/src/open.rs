use std::process::{Command, ExitStatus};

use anyhow::{Result, bail};

pub fn open_url(url: &str) -> Result<()> {
    if !is_github_url(url) {
        bail!("refusing to open a non-GitHub URL");
    }
    open_url_with(&SystemOpener, url, platform_openers())
}

#[cfg(target_os = "macos")]
fn platform_openers() -> &'static [&'static str] {
    &["open", "xdg-open"]
}

#[cfg(not(target_os = "macos"))]
fn platform_openers() -> &'static [&'static str] {
    &["xdg-open", "open"]
}

trait Opener {
    fn open(&self, program: &str, url: &str) -> std::io::Result<ExitStatus>;
}

#[derive(Debug, Clone, Copy)]
struct SystemOpener;

impl Opener for SystemOpener {
    fn open(&self, program: &str, url: &str) -> std::io::Result<ExitStatus> {
        Command::new(program).arg(url).status()
    }
}

fn open_url_with(opener: &impl Opener, url: &str, programs: &[&str]) -> Result<()> {
    let mut failures = Vec::new();
    for program in programs {
        match opener.open(program, url) {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => failures.push(format!("{program} exited with {status}")),
            Err(error) => failures.push(format!(
                "{:#}",
                anyhow::Error::new(error).context(format!("starting {program}"))
            )),
        }
    }
    bail!("could not open GitHub URL: {}", failures.join("; "))
}

fn is_github_url(url: &str) -> bool {
    if url.chars().any(char::is_control) || url.chars().any(char::is_whitespace) {
        return false;
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    scheme.eq_ignore_ascii_case("https")
        && authority.eq_ignore_ascii_case("github.com")
        && !rest.contains('\\')
        && has_valid_percent_encoding(rest)
}

fn has_valid_percent_encoding(value: &str) -> bool {
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let Some(high) = bytes.next() else {
                return false;
            };
            let Some(low) = bytes.next() else {
                return false;
            };
            if !high.is_ascii_hexdigit() || !low.is_ascii_hexdigit() {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::os::unix::process::ExitStatusExt as _;
    use std::sync::Mutex;

    use super::*;

    #[derive(Debug)]
    struct FakeOpener {
        outcomes: Mutex<VecDeque<std::io::Result<ExitStatus>>>,
        calls: Mutex<Vec<String>>,
    }

    impl Opener for FakeOpener {
        fn open(&self, program: &str, _url: &str) -> std::io::Result<ExitStatus> {
            self.calls.lock().unwrap().push(program.to_owned());
            self.outcomes.lock().unwrap().pop_front().unwrap()
        }
    }

    #[test]
    fn only_https_github_origins_are_accepted() {
        assert!(is_github_url("https://github.com/example/repo/pull/1"));
        assert!(is_github_url("HTTPS://GITHUB.COM/example/repo"));
        for url in [
            "http://github.com/example/repo",
            "https://github.com.evil.test/example/repo",
            "https://user@github.com/example/repo",
            "https://github.com:443/example/repo",
            "https://github.com/example repo",
            "https://github.com/example%2",
            "https://github.com/example%xx",
            "https://github.com\\@evil.test/example/repo",
            "https://github.com/example\0repo",
            "github.com/example/repo",
        ] {
            assert!(!is_github_url(url), "accepted {url}");
        }
    }

    #[test]
    fn a_failed_opener_falls_back_to_the_next_candidate() {
        let opener = FakeOpener {
            outcomes: Mutex::new(
                vec![
                    Ok(ExitStatus::from_raw(1 << 8)),
                    Ok(ExitStatus::from_raw(0)),
                ]
                .into(),
            ),
            calls: Mutex::new(Vec::new()),
        };
        open_url_with(
            &opener,
            "https://github.com/example/repo",
            &["first", "second"],
        )
        .unwrap();
        assert_eq!(opener.calls.lock().unwrap().as_slice(), ["first", "second"]);
    }
}
