use std::fmt;
use std::ops::Deref;
use std::path::{Component, Path, PathBuf};

/// A path wrapper that keeps absolute paths hidden from the LLM as much as possible.
///
/// Paths surfaced to the LLM should not leak the user's filesystem layout, so [`TenonPath`] pairs
/// the path with the working directory it is expressed against (the process working directory by
/// default).
pub struct TenonPath {
    path: String,
    working_directory: PathBuf,
}

impl TenonPath {
    /// The process working directory, falling back to `.` when unavailable.
    fn process_working_directory() -> PathBuf {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }

    /// Creates a TenonPath expressed against `working_directory`; `None` anchors to the process
    /// working directory.
    pub fn new(path: impl Into<String>, working_directory: Option<impl Into<PathBuf>>) -> Self {
        Self {
            path: path.into(),
            working_directory: match working_directory {
                Some(dir) => dir.into(),
                None => Self::process_working_directory(),
            },
        }
    }

    pub fn with_cwd(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            working_directory: Self::process_working_directory(),
        }
    }
}

impl Deref for TenonPath {
    type Target = Path;

    fn deref(&self) -> &Path {
        self.as_ref()
    }
}

impl AsRef<Path> for TenonPath {
    fn as_ref(&self) -> &Path {
        Path::new(&self.path)
    }
}

/// Expands a leading `~`/`~/` to `home`. Unset home and `~user` fall back to
/// the input unchanged.
fn expand_home(path: &str, home: Option<&str>) -> PathBuf {
    let Some(home) = home else {
        return PathBuf::from(path);
    };
    if path == "~" {
        PathBuf::from(home)
    } else if let Some(rest) = path.strip_prefix("~/") {
        PathBuf::from(home).join(rest)
    } else {
        PathBuf::from(path)
    }
}

impl From<TenonPath> for PathBuf {
    fn from(tenon: TenonPath) -> Self {
        // `.` resolves to the working directory itself; joining would keep a
        // trailing `.` component (`/project/.`)
        if tenon.path == "." {
            return tenon.working_directory;
        }
        let home = std::env::var("HOME").ok();
        let path = expand_home(&tenon.path, home.as_deref());
        if path.is_relative() {
            tenon.working_directory.join(path)
        } else {
            path
        }
    }
}

impl fmt::Display for TenonPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.rendered())
    }
}

impl TenonPath {
    fn rendered(&self) -> String {
        // `~`-form renders verbatim: std sees it as a relative path, so the
        // machinery below would `./`-prefix it
        if self.path.starts_with('~') {
            return self.path.clone();
        }

        let path = Path::new(&self.path);
        match (path.is_absolute(), self.working_directory.is_absolute()) {
            (true, true) => relative_between(path, &self.working_directory),
            // An absolute path cannot be expressed against a relative base
            (true, false) => self.path.clone(),
            (false, _) => anchored_relative(path),
        }
    }
}

/// Relativize an absolute `path` against an absolute `base`, component-wise:
/// `./rest` when `path` is at or under `base`, otherwise `..`-hops + rest.
///
/// Component comparison avoids the byte-prefix false matches of string
/// stripping (`/home/user` vs `/home/userx`).
fn relative_between(path: &Path, base: &Path) -> String {
    let path_comps: Vec<Component> = path.components().collect();
    let base_comps: Vec<Component> = base.components().collect();
    let common = path_comps
        .iter()
        .zip(base_comps.iter())
        .take_while(|(p, b)| p == b)
        .count();
    let ups = base_comps.len() - common;
    let rest: PathBuf = path_comps[common..].iter().collect();

    if ups == 0 {
        format!("./{}", rest.display())
    } else {
        format!("{}{}", "../".repeat(ups), rest.display())
    }
}

/// A stored path already relative to its working directory: `./rest` while it
/// stays inside, verbatim when it escapes via a leading `..`.
fn anchored_relative(path: &Path) -> String {
    let mut comps = path.components();
    match comps.next() {
        Some(Component::ParentDir) => path.display().to_string(),
        // Drop a leading `./` before prefixing our own
        Some(Component::CurDir) => format!("./{}", comps.collect::<PathBuf>().display()),
        _ => format!("./{}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_relative_inside_workdir() {
        let tenon = TenonPath::new("/project/src/main.rs", Some("/project"));
        assert_eq!(tenon.to_string(), "./src/main.rs");

        // Relative stored paths are anchored to the working directory
        let tenon = TenonPath::new("src/main.rs", Some("/project"));
        assert_eq!(tenon.to_string(), "./src/main.rs");

        // Path equal to the workdir renders as bare `./`
        let tenon = TenonPath::new("/project", Some("/project"));
        assert_eq!(tenon.to_string(), "./");
    }

    #[test]
    fn test_display_outside_workdir_resolves_relative() {
        // Sibling of the workdir resolves through its parent
        let tenon = TenonPath::new("/a/b/t.txt", Some("/a/b/c"));
        assert_eq!(tenon.to_string(), "../t.txt");

        // Deeper outside paths accumulate parent hops
        let tenon = TenonPath::new("/elsewhere/file.txt", Some("/project/sub"));
        assert_eq!(tenon.to_string(), "../../elsewhere/file.txt");

        // A stored relative path escaping the workdir keeps its `..` form
        let tenon = TenonPath::new("../elsewhere/file.txt", Some("/project"));
        assert_eq!(tenon.to_string(), "../elsewhere/file.txt");
    }

    #[test]
    fn test_display_prefix_is_component_wise() {
        // Byte-prefix stripping would wrongly render `./x/file` here;
        // component-wise resolution gives the sibling path
        let tenon = TenonPath::new("/home/userx/file", Some("/home/user"));
        assert_eq!(tenon.to_string(), "../userx/file");
    }

    #[test]
    fn test_display_verbatim_when_relative_unresolvable() {
        // An absolute path cannot be relativized against a relative workdir
        let tenon = TenonPath::new("/elsewhere/file.txt", Some("relative/base"));
        assert_eq!(tenon.to_string(), "/elsewhere/file.txt");
    }

    #[test]
    fn test_with_cwd_uses_cwd_as_working_directory() {
        let cwd = std::env::current_dir().expect("cwd readable in test environment");

        let tenon = TenonPath::with_cwd("some_file.rs");
        assert_eq!(tenon.to_string(), "./some_file.rs");
        let resolved: PathBuf = tenon.into();
        assert_eq!(resolved, cwd.join("some_file.rs"));

        // new(path, None) resolves the same as with_cwd
        let tenon = TenonPath::new("some_file.rs", None::<PathBuf>);
        let resolved: PathBuf = tenon.into();
        assert_eq!(resolved, cwd.join("some_file.rs"));
    }

    #[test]
    fn test_init_stores_input_verbatim() {
        assert_eq!(TenonPath::with_cwd("~/docs").path, "~/docs");
        assert_eq!(
            TenonPath::new("~/docs/f.rs", Some("/project")).path,
            "~/docs/f.rs"
        );
        assert_eq!(TenonPath::with_cwd("~").path, "~");

        // ~user and mid-string ~ are not tilde-form
        assert_eq!(TenonPath::with_cwd("~otheruser/foo").path, "~otheruser/foo");
        assert_eq!(TenonPath::with_cwd("/tmp/~weird").path, "/tmp/~weird");

        // Not pre-joined against the working directory
        assert_eq!(
            TenonPath::new("src/main.rs", Some("/project")).path,
            "src/main.rs"
        );
        assert_eq!(TenonPath::with_cwd("/etc/hosts").path, "/etc/hosts");
    }

    #[test]
    fn test_display_tilde_form_renders_verbatim() {
        // ~-form is home-anchored, not workdir-anchored
        let tenon = TenonPath::new("~/docs/f.rs", Some("/project"));
        assert_eq!(tenon.to_string(), "~/docs/f.rs");
    }

    #[test]
    fn test_from_joins_relative_against_workdir() {
        let tenon = TenonPath::new("src/main.rs", Some("/project"));
        let resolved: PathBuf = tenon.into();
        assert_eq!(resolved, PathBuf::from("/project/src/main.rs"));

        // Absolute stored paths pass through untouched
        let tenon = TenonPath::new("/elsewhere/f.txt", Some("/project"));
        let resolved: PathBuf = tenon.into();
        assert_eq!(resolved, PathBuf::from("/elsewhere/f.txt"));

        // Mid-string ~ is not tilde-form; passes through untouched
        let tenon = TenonPath::with_cwd("/tmp/~weird");
        let resolved: PathBuf = tenon.into();
        assert_eq!(resolved, PathBuf::from("/tmp/~weird"));

        // ~-form expands to $HOME, then passes through as absolute
        let home = std::env::var("HOME").unwrap_or("~".to_string());
        let tenon = TenonPath::new("~/docs/f.rs", Some("/project"));
        let resolved: PathBuf = tenon.into();
        assert_eq!(resolved, PathBuf::from(&home).join("docs/f.rs"));

        // ~ alone resolves to $HOME itself
        let tenon = TenonPath::new("~", Some("/project"));
        let resolved: PathBuf = tenon.into();
        assert_eq!(resolved, PathBuf::from(&home));

        // ~user is not expanded: joins against the workdir, matching the
        // tools' net behavior today
        let tenon = TenonPath::new("~otheruser/foo", Some("/project"));
        let resolved: PathBuf = tenon.into();
        assert_eq!(resolved, PathBuf::from("/project/~otheruser/foo"));
    }

    #[test]
    fn test_from_relative_working_directory() {
        // A relative workdir is used as-is, not resolved against cwd
        let resolved: PathBuf = TenonPath::new("rel/child.txt", Some("base")).into();
        assert_eq!(resolved, PathBuf::from("base/rel/child.txt"));
    }

    #[test]
    fn test_expand_home() {
        let home = Some("/home/u");

        assert_eq!(expand_home("~", home), PathBuf::from("/home/u"));
        assert_eq!(expand_home("~/docs", home), PathBuf::from("/home/u/docs"));

        // Unset $HOME: tilde-form stays literal instead of expanding
        assert_eq!(expand_home("~", None), PathBuf::from("~"));
        assert_eq!(expand_home("~/docs", None), PathBuf::from("~/docs"));

        assert_eq!(
            expand_home("src/main.rs", home),
            PathBuf::from("src/main.rs")
        );
        assert_eq!(
            expand_home("~otheruser/f", home),
            PathBuf::from("~otheruser/f")
        );
    }

    #[test]
    fn test_from_resolves_dot_to_working_directory() {
        // `.` must resolve to the workdir itself, not keep a joined `.` component
        let resolved: PathBuf = TenonPath::new(".", Some("/project")).into();
        assert_eq!(resolved, PathBuf::from("/project"));

        let resolved: PathBuf = TenonPath::with_cwd(".").into();
        assert_eq!(
            resolved,
            std::env::current_dir().expect("cwd readable in test environment")
        );
    }

    #[test]
    fn test_deref_and_as_ref_expose_stored_path() {
        let tenon = TenonPath::new("src/main.rs", Some("/project"));

        // Deref coercion
        let view: &Path = &tenon;
        assert_eq!(view, Path::new("src/main.rs"));

        assert_eq!(tenon.as_ref(), Path::new("src/main.rs"));
    }
}
