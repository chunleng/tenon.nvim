use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::DependencySource;
use crate::utils::path::TenonPath;
use rig::tool::ToolExecutionError;
use serde::Deserialize;

#[derive(Debug)]
pub enum PythonUvValidationError {
    LockfileNotFound(PathBuf),
    Validation(ToolExecutionError),
}

impl From<ToolExecutionError> for PythonUvValidationError {
    fn from(error: ToolExecutionError) -> Self {
        Self::Validation(error)
    }
}

impl From<PythonUvValidationError> for ToolExecutionError {
    fn from(error: PythonUvValidationError) -> Self {
        match error {
            PythonUvValidationError::LockfileNotFound(path) => Self::not_found(format!(
                "Not a uv project: uv.lock not found in current directory ({})",
                TenonPath::new(path.to_string_lossy(), path.parent())
            )),
            PythonUvValidationError::Validation(error) => error,
        }
    }
}

#[derive(Deserialize)]
struct UvLock {
    version: u32,
    #[serde(default)]
    package: Vec<LockedPackage>,
}

#[derive(Deserialize)]
struct LockedPackage {
    name: String,
    version: String,
}

pub fn validate_python_uv_dependency(
    dep_name: &str,
    dep_version: Option<&str>,
) -> Result<String, PythonUvValidationError> {
    let cwd = std::env::current_dir()
        .map_err(|e| ToolExecutionError::other(format!("Cannot get current directory: {e}")))?;
    validate_lockfile(&cwd, dep_name, dep_version)
}

fn normalize_distribution_name(name: &str) -> String {
    let mut normalized = String::with_capacity(name.len());
    for character in name.chars() {
        if matches!(character, '-' | '_' | '.') {
            if !normalized.ends_with('-') {
                normalized.push('-');
            }
        } else {
            normalized.push(character.to_ascii_lowercase());
        }
    }
    normalized
}

fn validate_lockfile(
    root: &Path,
    dep_name: &str,
    dep_version: Option<&str>,
) -> Result<String, PythonUvValidationError> {
    let path = root.join("uv.lock");
    let content = fs::read_to_string(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            PythonUvValidationError::LockfileNotFound(path.clone())
        } else {
            ToolExecutionError::other(format!("Failed to read uv.lock: {error}")).into()
        }
    })?;
    let lock: UvLock = toml::from_str(&content)
        .map_err(|e| ToolExecutionError::other(format!("Failed to parse uv.lock: {e}")))?;
    if lock.version != 1 {
        return Err(ToolExecutionError::other(format!(
            "Unsupported uv.lock format version: {}",
            lock.version
        ))
        .into());
    }

    let normalized = normalize_distribution_name(dep_name);
    let versions: BTreeSet<String> = lock
        .package
        .into_iter()
        .filter(|package| normalize_distribution_name(&package.name) == normalized)
        .map(|package| package.version)
        .collect();
    if versions.is_empty() {
        return Err(ToolExecutionError::not_found(format!(
            "Distribution '{dep_name}' not found in uv.lock. Use its distribution name, not its import name."
        ))
        .into());
    }
    let available = versions.iter().cloned().collect::<Vec<_>>().join(", ");
    match dep_version {
        Some(requested) if versions.contains(requested) => Ok(requested.to_string()),
        Some(requested) => Err(ToolExecutionError::invalid_args(format!(
            "Version mismatch: requested '{requested}' but uv.lock has: {available}"
        ))
        .into()),
        None if versions.len() > 1 => Err(ToolExecutionError::invalid_args(format!(
            "Multiple versions of '{dep_name}' in uv.lock: {available}. Specify which version to search."
        ))
        .into()),
        None => Ok(versions.into_iter().next().unwrap()),
    }
}

#[derive(Deserialize)]
struct InstalledDistribution {
    version: Option<String>,
    files: Vec<PathBuf>,
    source_roots: Vec<PathBuf>,
}

// -S prevents site initialization from executing dependency code through .pth files.
// Explicit sysconfig bases also locate virtualenv packages on Python versions where
// -S leaves sys.prefix pointing at the base interpreter.
const INSTALLED_METADATA_SCRIPT: &str = r#"
import importlib.metadata
import json
import pathlib
import sys
import sysconfig

root, name = sys.argv[1:]
paths = sysconfig.get_paths(vars={'base': root, 'platbase': root})
source_roots = sorted({paths['purelib'], paths['platlib']})
def normalize(value):
    import re
    return re.sub(r'[-_.]+', '-', value).lower()

matches = [dist for dist in importlib.metadata.distributions(path=source_roots)
           if normalize(dist.metadata.get('Name', '')) == normalize(name)]
if len(matches) > 1:
    raise RuntimeError('Multiple installed distributions match ' + name + '; run uv sync')
dist = matches[0] if matches else None
print(json.dumps({
    'version': dist.version if dist else None,
    'files': [str(pathlib.Path(dist.locate_file(file)).absolute())
              for file in (dist.files or [])] if dist else [],
    'source_roots': source_roots,
}))
"#;

fn project_environment(root: &Path, override_path: Option<&OsStr>) -> PathBuf {
    match override_path {
        Some(path) => root.join(path),
        None => root.join(".venv"),
    }
}

pub(super) fn resolve_python_uv_source(
    dep_name: &str,
    dep_version: &str,
) -> Result<DependencySource, ToolExecutionError> {
    let root = std::env::current_dir()
        .map_err(|e| ToolExecutionError::other(format!("Cannot get current directory: {e}")))?;
    let environment =
        project_environment(&root, std::env::var_os("UV_PROJECT_ENVIRONMENT").as_deref());
    query_installed_source(&environment, dep_name, dep_version)
}

fn query_installed_source(
    environment: &Path,
    dep_name: &str,
    dep_version: &str,
) -> Result<DependencySource, ToolExecutionError> {
    let interpreter = environment.join(if cfg!(windows) {
        "Scripts/python.exe"
    } else {
        "bin/python"
    });
    if !interpreter.is_file() {
        return Err(ToolExecutionError::not_found(format!(
            "Python project environment interpreter not found: {}. Run uv sync.",
            TenonPath::new(interpreter.to_string_lossy(), Some(environment))
        )));
    }
    let output = Command::new(&interpreter)
        .args(["-I", "-S", "-B", "-c", INSTALLED_METADATA_SCRIPT])
        .arg(environment)
        .arg(dep_name)
        .output()
        .map_err(|e| {
            ToolExecutionError::other(format!(
                "Cannot query Python project environment {}: {e}. Run uv sync.",
                TenonPath::with_cwd(environment.to_string_lossy())
            ))
        })?;
    if !output.status.success() {
        return Err(ToolExecutionError::other(format!(
            "Failed to query installed Python distribution metadata: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let metadata: InstalledDistribution = serde_json::from_slice(&output.stdout).map_err(|e| {
        ToolExecutionError::other(format!(
            "Invalid installed Python distribution metadata: {e}"
        ))
    })?;
    installed_source(metadata, dep_name, dep_version)
}

fn installed_source(
    metadata: InstalledDistribution,
    dep_name: &str,
    dep_version: &str,
) -> Result<DependencySource, ToolExecutionError> {
    let Some(installed_version) = metadata.version else {
        return Err(ToolExecutionError::not_found(format!(
            "Distribution '{dep_name}' is not installed in the selected project environment. Run uv sync."
        )));
    };
    if installed_version != dep_version {
        return Err(ToolExecutionError::invalid_args(format!(
            "Installed version mismatch for '{dep_name}': uv.lock selected '{dep_version}', but the environment has '{installed_version}'. Run uv sync."
        )));
    }
    let roots: Vec<PathBuf> = metadata
        .source_roots
        .iter()
        .filter_map(|path| fs::canonicalize(path).ok())
        .collect();
    let files: BTreeSet<PathBuf> = metadata
        .files
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "py"))
        .filter_map(|path| fs::canonicalize(path).ok())
        .filter(|path| {
            roots.iter().any(|root| {
                path.strip_prefix(root).is_ok_and(|relative| {
                    !relative.components().any(|component| {
                        let name = component.as_os_str().to_string_lossy();
                        name.ends_with(".dist-info") || name.ends_with(".egg-info")
                    })
                })
            }) && fs::read_to_string(path).is_ok()
        })
        .collect();
    let mut root = files
        .first()
        .and_then(|file| file.parent())
        .ok_or_else(|| {
            ToolExecutionError::not_found(format!(
                "No readable Python implementation source recorded for '{dep_name}' {dep_version}. Installed metadata may lack source files, or the implementation may be compiled-only."
            ))
        })?
        .to_path_buf();
    for file in &files {
        while !file.starts_with(&root) {
            root.pop();
        }
    }
    let allowed_files = files
        .into_iter()
        .map(|path| {
            path.to_str()
                .map(|path| TenonPath::new(path, Some(&root)))
                .ok_or_else(|| {
                    ToolExecutionError::other("Dependency source path is not valid UTF-8")
                })
        })
        .collect::<Result<_, _>>()?;
    Ok(DependencySource {
        root,
        allowed_files: Some(allowed_files),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Project(PathBuf);

    impl Project {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("tenon-uv-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn lock(&self, content: &str) {
            fs::write(self.0.join("uv.lock"), content).unwrap();
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn selects_only_project_environment_paths() {
        let project = Project::new();
        assert_eq!(
            project_environment(&project.0, None),
            project.0.join(".venv")
        );
        assert_eq!(
            project_environment(&project.0, Some(OsStr::new("custom-env"))),
            project.0.join("custom-env")
        );
        let absolute = project.0.join("elsewhere");
        assert_eq!(
            project_environment(&project.0, Some(absolute.as_os_str())),
            absolute
        );
        let error = query_installed_source(&project.0.join(".venv"), "example", "1.0")
            .unwrap_err()
            .to_string();
        assert!(error.contains("interpreter not found"));
        assert!(error.contains("uv sync"));
        assert!(!error.contains(project.0.to_str().unwrap()), "got: {error}");
    }

    #[test]
    fn checks_installed_version_before_source_discovery() {
        for (version, expected) in [
            (None, "not installed"),
            (Some("2.0".to_string()), "Installed version mismatch"),
        ] {
            let error = installed_source(
                InstalledDistribution {
                    version,
                    files: vec![],
                    source_roots: vec![],
                },
                "example",
                "1.0",
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains(expected));
            assert!(error.contains("uv sync"));
        }
        let error = installed_source(
            InstalledDistribution {
                version: Some("1.0".to_string()),
                files: vec![],
                source_roots: vec![],
            },
            "example",
            "1.0",
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("No readable Python implementation source"));
    }

    #[test]
    fn discovers_only_recorded_readable_implementation_files_across_modules() {
        let project = Project::new();
        let site = project.0.join("site-packages");
        fs::create_dir_all(site.join("shared")).unwrap();
        fs::create_dir_all(site.join("example.dist-info")).unwrap();
        let owned = site.join("shared/owned.py");
        let module = site.join("another.py");
        let unrelated = site.join("shared/unrelated.py");
        let outside = project.0.join("outside.py");
        let metadata_file = site.join("example.dist-info/setup.py");
        let compiled = site.join("extension.so");
        for path in [
            &owned,
            &module,
            &unrelated,
            &outside,
            &metadata_file,
            &compiled,
        ] {
            fs::write(path, "raise RuntimeError('must not execute')").unwrap();
        }
        let unreadable = site.join("binary.py");
        fs::write(&unreadable, [0xff]).unwrap();
        let source = installed_source(
            InstalledDistribution {
                version: Some("1.0".to_string()),
                files: vec![
                    owned.clone(),
                    module.clone(),
                    outside,
                    metadata_file,
                    compiled,
                    unreadable,
                    site.join("missing.py"),
                ],
                source_roots: vec![site.clone()],
            },
            "example",
            "1.0",
        )
        .unwrap();
        assert_eq!(source.root, fs::canonicalize(site).unwrap());
        let files: BTreeSet<PathBuf> = source
            .allowed_files
            .unwrap()
            .into_iter()
            .map(PathBuf::from)
            .collect();
        assert_eq!(
            files,
            BTreeSet::from([
                fs::canonicalize(owned).unwrap(),
                fs::canonicalize(module).unwrap()
            ])
        );
        assert!(!files.contains(&fs::canonicalize(unrelated).unwrap()));
    }

    #[cfg(unix)]
    #[test]
    fn excludes_recorded_symlinks_leading_outside_source_roots() {
        let project = Project::new();
        let site = project.0.join("site-packages");
        fs::create_dir(&site).unwrap();
        let outside = project.0.join("outside.py");
        fs::write(&outside, "value = 42").unwrap();
        let link = site.join("linked.py");
        std::os::unix::fs::symlink(outside, &link).unwrap();
        let error = installed_source(
            InstalledDistribution {
                version: Some("1.0".to_string()),
                files: vec![link],
                source_roots: vec![site],
            },
            "example",
            "1.0",
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("No readable Python implementation source"));
    }

    #[test]
    fn resolves_normalized_transitive_distribution_and_deduplicates_versions() {
        let project = Project::new();
        project.lock(
            r#"version = 1
[[package]]
name = "root"
version = "0.1.0"
[[package]]
name = "beautiful-soup4"
version = "4.13.0"
[[package]]
name = "beautiful-soup4"
version = "4.13.0"
"#,
        );
        assert_eq!(
            validate_lockfile(&project.0, "Beautiful._Soup4", None).unwrap(),
            "4.13.0"
        );
        let error = validate_lockfile(&project.0, "bs4", None).unwrap_err();
        assert!(
            ToolExecutionError::from(error)
                .to_string()
                .contains("not found in uv.lock")
        );
        let error = validate_lockfile(&project.0, "beautiful-soup4", Some("0.0.0")).unwrap_err();
        assert!(
            ToolExecutionError::from(error)
                .to_string()
                .contains("Version mismatch")
        );
    }

    #[test]
    fn requires_explicit_version_for_multiple_distinct_locked_versions() {
        let project = Project::new();
        project.lock(
            r#"version = 1
[[package]]
name = "example"
version = "1.0"
[[package]]
name = "example"
version = "2.0"
"#,
        );
        let error = validate_lockfile(&project.0, "example", None).unwrap_err();
        assert!(
            ToolExecutionError::from(error)
                .to_string()
                .contains("Multiple versions")
        );
        assert_eq!(
            validate_lockfile(&project.0, "example", Some("2.0")).unwrap(),
            "2.0"
        );
    }

    #[test]
    fn only_absent_lockfile_returns_lockfile_not_found() {
        let project = Project::new();
        assert!(matches!(
            validate_lockfile(&project.0, "example", None),
            Err(PythonUvValidationError::LockfileNotFound(path)) if path == project.0.join("uv.lock")
        ));
        for content in [
            "invalid [",
            "version = 1\n[[package]]\nname = 'example'",
            "version = 2",
        ] {
            project.lock(content);
            let error = validate_lockfile(&project.0, "example", None).unwrap_err();
            assert!(matches!(error, PythonUvValidationError::Validation(_)));
        }
        project.lock("version = 1\n[[package]]\nname = 'example'\nversion = '1.0'");
        let member = project.0.join("member");
        fs::create_dir(&member).unwrap();
        let error = validate_lockfile(&member, "example", None).unwrap_err();
        assert!(matches!(
            &error,
            PythonUvValidationError::LockfileNotFound(path) if path == &member.join("uv.lock")
        ));
        let error = ToolExecutionError::from(error).to_string();
        assert!(error.contains("uv.lock not found in current directory"));
        assert!(!error.contains(project.0.to_str().unwrap()), "got: {error}");
        fs::remove_file(project.0.join("uv.lock")).unwrap();
        fs::create_dir(project.0.join("uv.lock")).unwrap();
        assert!(matches!(
            validate_lockfile(&project.0, "example", None),
            Err(PythonUvValidationError::Validation(_))
        ));
    }
}
