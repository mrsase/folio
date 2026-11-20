//! Path handling shared by every entry point: quick-open, drag-drop, recents
//! and the command line. One normalizer, so `~/x`, `./x` and `/abs/x` compare
//! consistently.

use std::fmt;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// `~user` is not supported; only the current user's home.
    UnsupportedUser(String),
    /// The path contains no home directory to expand against.
    NoHome,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathError::UnsupportedUser(user) => {
                write!(f, "`{user}` is not supported; use `~` or `~/…`")
            }
            PathError::NoHome => write!(f, "HOME is not set, so `~` cannot be expanded"),
        }
    }
}

impl std::error::Error for PathError {}

/// Expand a leading `~` or `~/`. Other paths are returned unchanged.
pub fn expand_home(input: &str, home: Option<&Path>) -> Result<PathBuf, PathError> {
    if input == "~" {
        return home.map(Path::to_path_buf).ok_or(PathError::NoHome);
    }
    if let Some(rest) = input.strip_prefix("~/") {
        return home.map(|home| home.join(rest)).ok_or(PathError::NoHome);
    }
    if input.starts_with('~') {
        return Err(PathError::UnsupportedUser(input.to_string()));
    }
    Ok(PathBuf::from(input))
}

/// Resolve a typed path against `base` (the root, or the active document's
/// folder), expand `~`, normalize `.`/`..`, and canonicalize when the path
/// exists. A nonexistent path is returned as a normalized absolute path so the
/// caller can report it without guessing at symlinks.
pub fn resolve(
    input: &str,
    base: Option<&Path>,
    home: Option<&Path>,
) -> Result<PathBuf, PathError> {
    let expanded = expand_home(input, home)?;
    let joined = if expanded.is_absolute() {
        expanded
    } else {
        match base {
            Some(base) => base.join(expanded),
            None => std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("/"))
                .join(expanded),
        }
    };
    let lexical = normalize_lexical(&joined);
    Ok(if lexical.exists() {
        std::fs::canonicalize(&lexical).unwrap_or(lexical)
    } else {
        lexical
    })
}

/// Remove `.` components and fold `..` lexically. This does not consult the
/// filesystem; callers that need symlink-aware identity use [`canonical`].
pub fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() && !path.is_absolute() {
                    out.push("..");
                }
            }
            Component::Normal(name) => out.push(name),
        }
    }
    out
}

/// Canonicalize if the path exists, otherwise return the lexical form.
pub fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| normalize_lexical(path))
}

/// Whether `path` is inside `root`, comparing canonical forms when possible.
pub fn is_within(root: &Path, path: &Path) -> bool {
    canonical(path).starts_with(canonical(root))
}

/// Display form: `/private/tmp` becomes `/tmp`, the home prefix becomes `~`.
pub fn display(path: &Path, home: Option<&Path>) -> String {
    let mut short = path.to_path_buf();
    if let Ok(rest) = path.strip_prefix("/private") {
        let first = rest
            .components()
            .next()
            .and_then(|component| component.as_os_str().to_str())
            .unwrap_or_default();
        if matches!(first, "tmp" | "var" | "etc") {
            short = Path::new("/").join(rest);
        }
    }
    if let Some(home) = home
        && let Ok(rest) = short.strip_prefix(home)
    {
        if rest.as_os_str().is_empty() {
            return "~".to_string();
        }
        return format!("~/{}", rest.display());
    }
    short.display().to_string()
}

/// Split a quick-open query into "path intent" and plain text. A query that
/// starts with `~` or `/`, or contains a separator, is treated as a path.
pub fn looks_like_path(query: &str) -> bool {
    let query = query.trim();
    query.starts_with('~') || query.starts_with('/') || query.contains('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/Users/example")
    }

    #[test]
    fn expands_tilde_forms() {
        assert_eq!(expand_home("~", Some(&home())).unwrap(), home());
        assert_eq!(
            expand_home("~/docs/a.md", Some(&home())).unwrap(),
            home().join("docs/a.md")
        );
        assert!(matches!(
            expand_home("~other/x", Some(&home())),
            Err(PathError::UnsupportedUser(_))
        ));
        assert!(expand_home("~", None).is_err());
        assert_eq!(
            expand_home("plain/relative", Some(&home())).unwrap(),
            PathBuf::from("plain/relative")
        );
    }

    #[test]
    fn folds_dot_and_dotdot_lexically() {
        assert_eq!(
            normalize_lexical(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
        assert_eq!(
            normalize_lexical(Path::new("a/../../b")),
            PathBuf::from("../b")
        );
        assert_eq!(normalize_lexical(Path::new("/")), PathBuf::from("/"));
    }

    #[test]
    fn resolves_relative_against_the_base() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir(base.join("notes")).unwrap();
        let resolved = resolve("notes/../notes/a.md", Some(base), Some(&home())).unwrap();
        assert_eq!(
            resolved,
            std::fs::canonicalize(base.join("notes/a.md")).unwrap_or(base.join("notes/a.md"))
        );
    }

    #[test]
    fn canonical_paths_compare_across_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(real.join("doc.md"), "# hi").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(is_within(&real, &link.join("doc.md")));
        assert_eq!(canonical(&link), std::fs::canonicalize(&real).unwrap());
    }

    #[test]
    fn displays_short_paths() {
        assert_eq!(
            display(Path::new("/private/tmp/notes.md"), Some(&home())),
            "/tmp/notes.md"
        );
        assert_eq!(
            display(&home().join("docs/a.md"), Some(&home())),
            "~/docs/a.md"
        );
        assert_eq!(display(&home(), Some(&home())), "~");
        assert_eq!(
            display(Path::new("/private/var/folders/x"), Some(&home())),
            "/var/folders/x"
        );
    }

    #[test]
    fn path_intent_is_recognizable() {
        assert!(looks_like_path("~/docs/a.md"));
        assert!(looks_like_path("/tmp/a.md"));
        assert!(looks_like_path("docs/a.md"));
        assert!(!looks_like_path("a.md"));
        assert!(!looks_like_path("readme"));
    }
}
