//! Shared lexical validation and cooperative filesystem checks for managed documents.
use std::fs;
use std::path::{Path, PathBuf};

use crate::{Result, invalid, io};

/// Validate the exact spelling, before `Path::components` can normalize it.
/// Managed paths use forward slashes and visible Unicode components only.
pub fn validate_relative(relative: &Path) -> Result<()> {
    let text = relative
        .to_str()
        .ok_or_else(|| invalid("document path must be Unicode"))?;
    if text.is_empty()
        || text.len() > 4096
        || text.contains(['\\', ':'])
        || text.chars().any(char::is_control)
        || text
            .split('/')
            .any(|part| part.is_empty() || part.starts_with('.') || part.len() > 255)
    {
        return Err(invalid(
            "document path must use nonempty visible relative components, without traversal, control characters, colons or backslashes",
        ));
    }
    Ok(())
}

/// A goal agreement belongs to the project's briefs directory.
pub fn validate_brief(relative: &Path) -> Result<()> {
    validate_markdown(relative)?;
    if !relative.starts_with("briefs") || relative.components().count() < 2 {
        return Err(invalid(
            "brief must name a project-relative briefs/<name>.md file",
        ));
    }
    Ok(())
}

pub fn validate_markdown(relative: &Path) -> Result<()> {
    validate_relative(relative)?;
    if relative
        .extension()
        .is_none_or(|extension| extension != "md")
    {
        return Err(invalid("document paths must name Markdown (.md) files"));
    }
    Ok(())
}

/// Resolve without creating anything or following symlinks within the selected root.
/// External ancestors belong to the caller; this is not a hostile-writer sandbox.
pub fn resolve(root: &Path, relative: &Path) -> Result<PathBuf> {
    validate_relative(relative)?;
    if !root.is_absolute() {
        return Err(invalid("document root must be absolute"));
    }
    let mut path = root.to_owned();
    reject_symlink(&path)?;
    for component in relative.components() {
        path.push(component);
        reject_symlink(&path)?;
    }
    Ok(path)
}

pub(crate) fn reject_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(invalid(format!(
            "symlink document path refused: {}",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io("inspect document path", path, error)),
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_markdown, validate_relative};
    use std::path::Path;

    #[test]
    fn validates_spelling_before_path_normalization() {
        for name in [
            "",
            "/brief.md",
            "./brief.md",
            "briefs/./brief.md",
            "briefs//brief.md",
            "briefs/",
            "briefs/../brief.md",
            "briefs/.hidden.md",
            "briefs\\brief.md",
            "C:brief.md",
            "briefs/a\nb.md",
            "briefs/a\0b.md",
        ] {
            assert!(validate_relative(Path::new(name)).is_err(), "{name:?}");
        }
        assert!(validate_relative(Path::new(&"a".repeat(256))).is_err());
        for name in [
            "briefs/plan.md",
            "knowledge/café.md",
            "outputs/report/sha256-ab/report.txt",
        ] {
            validate_relative(Path::new(name)).unwrap();
        }
        assert!(validate_markdown(Path::new("briefs/plan.txt")).is_err());
        validate_markdown(Path::new("briefs/plan.md")).unwrap();
    }
}
