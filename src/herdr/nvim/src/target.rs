use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};

/// A place in a file, as a click or a picker hands it over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub path: PathBuf,
    pub line: u32,
    pub column: u32,
}

impl Target {
    /// The top of a file, which is where a picker lands.
    pub fn at(path: PathBuf) -> Self {
        Self {
            path,
            line: 1,
            column: 1,
        }
    }

    /// Parses `file://…`, `~/…`, an absolute path, or a path relative to
    /// `base`, each optionally followed by `:line` or `:line:col`.
    pub fn parse(raw: &str, base: Option<&Path>, home: Option<&Path>) -> Result<Self> {
        let encoded = local_file_url_path(raw)?;
        let syntax = encoded.unwrap_or(raw);
        let decode =
            |value| encoded.map_or_else(|| PathBuf::from(value), |_| percent_decode(value));

        // A real filename wins over the optional position grammar. This is
        // the only non-lossy interpretation of an existing `report:12`; a
        // caller that means line 12 of `report` still works when that longer
        // filename does not exist.
        let whole = resolve_path(decode(syntax), base, home)?;
        if whole.is_file() {
            return Ok(Self::at(whole));
        }

        // Position separators belong to the encoded input syntax, not the
        // decoded filename. `file:///tmp/report%3A12` names `report:12`.
        let (path, line, column) = split_position(syntax);
        let path = resolve_path(decode(path), base, home)?;
        if !path.is_file() {
            bail!("no such file: {}", path.display());
        }
        Ok(Self { path, line, column })
    }
}

fn resolve_path(path: PathBuf, base: Option<&Path>, home: Option<&Path>) -> Result<PathBuf> {
    let display = path.to_string_lossy();
    if display.starts_with('~') && display != "~" && !display.starts_with("~/") {
        bail!("~user paths are not supported: {display}");
    }

    Ok(if let Ok(rest) = path.strip_prefix("~") {
        let home = home.context("a ~ path with no HOME to resolve it against")?;
        home.join(rest)
    } else if path.is_absolute() {
        path
    } else {
        let base = base.context("a relative path with no directory to resolve it against")?;
        base.join(path)
    })
}

/// Returns the encoded path of a local file URL. A non-empty remote authority
/// is not a local path and must never be silently resolved beneath the pane's
/// cwd.
fn local_file_url_path(raw: &str) -> Result<Option<&str>> {
    let Some(rest) = raw.strip_prefix("file://") else {
        return Ok(None);
    };
    if rest.starts_with('/') {
        return Ok(Some(rest));
    }
    if let Some((authority, _)) = rest.split_once('/')
        && authority.eq_ignore_ascii_case("localhost")
    {
        return Ok(rest.strip_prefix(authority));
    }
    bail!("file URL has a non-local authority")
}

/// Peels a trailing `:line[:col]` by hand rather than with a pattern, so a
/// path that legitimately contains a colon is not mangled.
fn split_position(raw: &str) -> (&str, u32, u32) {
    let mut path = raw;
    let mut line = 1;
    let mut column = 1;

    if let Some((head, tail)) = path.rsplit_once(':')
        && let Ok(number) = tail.parse::<u32>()
    {
        path = head;
        line = number;
        if let Some((head, tail)) = path.rsplit_once(':')
            && let Ok(number) = tail.parse::<u32>()
        {
            path = head;
            column = line;
            line = number;
        }
    }
    (path, line.max(1), column.max(1))
}

/// `%XX` as OSC 8 encodes it. The result is an operating-system string rather
/// than UTF-8: URI escapes can faithfully name every Unix path. Anything that
/// is not a well-formed escape is left alone, because a literal `%` in a
/// filename is legal.
fn percent_decode(raw: &str) -> PathBuf {
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = raw
            .get(index + 1..index + 3)
            .filter(|_| bytes.get(index) == Some(&b'%'))
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        if let Some(byte) = hex {
            out.push(byte);
            index += 3;
        } else {
            if let Some(&byte) = bytes.get(index) {
                out.push(byte);
            }
            index += 1;
        }
    }
    PathBuf::from(OsString::from_vec(out))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn a_bare_path_has_no_position() {
        assert_eq!(split_position("/a/b.rs"), ("/a/b.rs", 1, 1));
    }

    #[test]
    fn one_number_is_a_line_and_two_are_line_then_column() {
        assert_eq!(split_position("/a/b.rs:42"), ("/a/b.rs", 42, 1));
        assert_eq!(split_position("/a/b.rs:42:7"), ("/a/b.rs", 42, 7));
    }

    #[test]
    fn a_colon_inside_a_filename_survives() {
        assert_eq!(split_position("/a/od:d.rs"), ("/a/od:d.rs", 1, 1));
        assert_eq!(split_position("/a/od:d.rs:9"), ("/a/od:d.rs", 9, 1));
    }

    #[test]
    fn a_zero_line_is_clamped_because_nvim_counts_from_one() {
        assert_eq!(split_position("/a/b.rs:0"), ("/a/b.rs", 1, 1));
    }

    #[test]
    fn percent_escapes_decode_and_stray_percents_do_not() {
        assert_eq!(percent_decode("/a/b%20c.rs"), Path::new("/a/b c.rs"));
        assert_eq!(percent_decode("/a/100%25.rs"), Path::new("/a/100%.rs"));
        assert_eq!(percent_decode("/a/50%off"), Path::new("/a/50%off"));
        assert_eq!(percent_decode("/a/b%"), Path::new("/a/b%"));
    }

    #[test]
    fn a_backslash_is_not_an_escape() {
        assert_eq!(percent_decode(r"/a/b\nc.rs"), Path::new(r"/a/b\nc.rs"));
    }

    #[test]
    fn multibyte_percent_escapes_recombine() {
        assert_eq!(percent_decode("/a/%E2%9C%93.rs"), Path::new("/a/✓.rs"));
    }

    #[test]
    fn percent_encoded_colon_is_part_of_the_path_not_a_position() {
        let (path, line, column) = split_position("/a/report%3A12");
        assert_eq!(percent_decode(path), Path::new("/a/report:12"));
        assert_eq!((line, column), (1, 1));
    }

    #[test]
    fn file_urls_accept_only_the_local_machine() {
        assert_eq!(
            local_file_url_path("file:///tmp/a").unwrap(),
            Some("/tmp/a")
        );
        assert_eq!(
            local_file_url_path("file://LOCALHOST/tmp/a").unwrap(),
            Some("/tmp/a")
        );
        assert!(local_file_url_path("file://remote.example/tmp/a").is_err());
        assert_eq!(local_file_url_path("/tmp/a").unwrap(), None);
    }

    #[test]
    fn percent_decoding_preserves_non_utf8_path_bytes() {
        use std::os::unix::ffi::OsStrExt as _;

        assert_eq!(
            percent_decode("/a/%FF.rs").as_os_str().as_bytes(),
            b"/a/\xff.rs"
        );
    }

    #[test]
    fn parse_keeps_an_encoded_colon_in_an_existing_filename() {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "herdr-nvim-target-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("report:12");
        fs::write(&path, b"").unwrap();

        let raw = format!("file://{}/report%3A12", dir.display());
        let target = Target::parse(&raw, None, None).unwrap();
        assert_eq!(target, Target::at(path));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn an_existing_numeric_colon_filename_wins_over_a_position() {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "herdr-nvim-target-colon-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("report:12");
        fs::write(&path, b"").unwrap();

        let target = Target::parse(path.to_str().unwrap(), None, None).unwrap();
        assert_eq!(target, Target::at(path));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_numeric_suffix_is_a_position_when_the_longer_file_does_not_exist() {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "herdr-nvim-target-position-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("report");
        fs::write(&path, b"").unwrap();

        let target = Target::parse(&format!("{}:12:4", path.display()), None, None).unwrap();
        assert_eq!(
            target,
            Target {
                path,
                line: 12,
                column: 4,
            }
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn tilde_user_paths_are_rejected_instead_of_treated_as_relative() {
        let error = Target::parse(
            "~someone/file",
            Some(Path::new("/tmp")),
            Some(Path::new("/home/current")),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("~user paths are not supported"));
    }
}
