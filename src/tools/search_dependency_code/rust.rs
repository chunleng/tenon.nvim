use std::fs;
use std::path::PathBuf;

use rig::tool::ToolExecutionError;

pub fn validate_rust_dependency(
    dep_name: &str,
    dep_version: Option<&str>,
) -> Result<String, ToolExecutionError> {
    let cwd = std::env::current_dir()
        .map_err(|e| ToolExecutionError::other(format!("Cannot get current directory: {}", e)))?;

    // Detect Rust project
    let cargo_toml_path = cwd.join("Cargo.toml");
    if !cargo_toml_path.exists() {
        return Err(ToolExecutionError::not_found(
            "Not a Rust project: Cargo.toml not found in current directory".to_string(),
        ));
    }

    // Verify dependency exists in Cargo.toml
    let cargo_toml_content = fs::read_to_string(&cargo_toml_path)
        .map_err(|e| ToolExecutionError::other(format!("Failed to read Cargo.toml: {}", e)))?;
    let cargo_toml: toml::Value = toml::from_str(&cargo_toml_content)
        .map_err(|e| ToolExecutionError::other(format!("Failed to parse Cargo.toml: {}", e)))?;

    let dep_sections = ["dependencies", "dev-dependencies", "build-dependencies"];
    let dep_found = dep_sections.iter().any(|section| {
        cargo_toml
            .get(section)
            .and_then(|v| v.as_table())
            .is_some_and(|deps| deps.contains_key(dep_name))
    });

    if !dep_found {
        return Err(ToolExecutionError::not_found(format!(
            "Dependency '{}' not found in Cargo.toml \
             (checked dependencies, dev-dependencies, build-dependencies)",
            dep_name
        )));
    }

    // Resolve exact version from Cargo.lock
    let cargo_lock_path = cwd.join("Cargo.lock");
    let cargo_lock_content = fs::read_to_string(&cargo_lock_path)
        .map_err(|e| ToolExecutionError::other(format!("Failed to read Cargo.lock: {}", e)))?;
    let cargo_lock: toml::Value = toml::from_str(&cargo_lock_content)
        .map_err(|e| ToolExecutionError::other(format!("Failed to parse Cargo.lock: {}", e)))?;

    let locked_versions: Vec<String> = cargo_lock
        .get("package")
        .and_then(|v| v.as_array())
        .map(|packages| {
            packages
                .iter()
                .filter_map(|pkg| {
                    if pkg.get("name").and_then(|v| v.as_str()) == Some(dep_name) {
                        pkg.get("version")
                            .and_then(|v| v.as_str())
                            .map(String::from)
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    if locked_versions.is_empty() {
        return Err(ToolExecutionError::not_found(format!(
            "Dependency '{}' not found in Cargo.lock. \
             Run 'cargo build' to generate or update it.",
            dep_name
        )));
    }

    match dep_version {
        Some(requested) => {
            if !locked_versions.iter().any(|v| v == requested) {
                let versions_str = locked_versions.join(", ");
                return Err(ToolExecutionError::invalid_args(format!(
                    "Version mismatch: requested '{}' but Cargo.lock has: {}",
                    requested, versions_str
                )));
            }
            Ok(requested.to_string())
        }
        None => {
            if locked_versions.len() > 1 {
                let versions_str = locked_versions.join(", ");
                return Err(ToolExecutionError::invalid_args(format!(
                    "Multiple versions of '{}' in Cargo.lock: {}. \
                     Specify which version to search.",
                    dep_name, versions_str
                )));
            }
            Ok(locked_versions[0].clone())
        }
    }
}

pub fn resolve_rust_source_path(
    dep_name: &str,
    dep_version: &str,
) -> Result<PathBuf, ToolExecutionError> {
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .output()
        .map_err(|e| ToolExecutionError::other(format!("Failed to run cargo metadata: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ToolExecutionError::other(format!(
            "cargo metadata failed: {}",
            stderr.trim()
        )));
    }

    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|e| {
        ToolExecutionError::other(format!("Failed to parse cargo metadata output: {}", e))
    })?;

    let manifest_path = metadata
        .get("packages")
        .and_then(|v| v.as_array())
        .and_then(|packages| {
            packages.iter().find_map(|pkg| {
                let name_match = pkg.get("name").and_then(|v| v.as_str()) == Some(dep_name);
                let version_match =
                    pkg.get("version").and_then(|v| v.as_str()) == Some(dep_version);
                if name_match && version_match {
                    pkg.get("manifest_path")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                } else {
                    None
                }
            })
        })
        .ok_or_else(|| {
            ToolExecutionError::not_found(format!(
                "Dependency {} {} not found in cargo metadata",
                dep_name, dep_version
            ))
        })?;

    let source_dir = PathBuf::from(&manifest_path)
        .parent()
        .ok_or_else(|| {
            ToolExecutionError::other(format!(
                "Cannot determine parent directory of manifest_path: {}",
                manifest_path
            ))
        })?
        .to_path_buf();

    if !source_dir.exists() {
        return Err(ToolExecutionError::not_found(format!(
            "Dependency source directory not found: {}",
            source_dir.display()
        )));
    }

    Ok(source_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rust_resolves_version_from_lock() {
        let version = validate_rust_dependency("toml", None).unwrap();
        assert_eq!(
            version, "0.8.23",
            "Should resolve exact version from Cargo.lock"
        );
    }

    #[test]
    fn validate_then_resolve_rust_happy_path() {
        let version = validate_rust_dependency("toml", None).unwrap();
        let path = resolve_rust_source_path("toml", &version).unwrap();
        assert!(
            path.exists(),
            "Resolved path should exist: {}",
            path.display()
        );
        assert!(path.is_dir(), "Resolved path should be a directory");
        assert!(
            path.to_string_lossy()
                .contains(&format!("toml-{}", version)),
            "Path should contain dep-version: {}",
            path.display()
        );
    }

    #[test]
    fn validate_rust_version_mismatch() {
        let result = validate_rust_dependency("toml", Some("0.0.0"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Version mismatch"),
            "Should report version mismatch, got: {}",
            err
        );
    }

    #[test]
    fn validate_rust_dependency_not_found() {
        let result = validate_rust_dependency("nonexistent_dep_xyz", Some("1.0.0"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not found in Cargo.toml"),
            "Should report dependency not found, got: {}",
            err
        );
    }

    #[test]
    fn resolve_git_dependency() {
        // nvim-oxi is a real git dependency in this project
        let version = validate_rust_dependency("nvim-oxi", None).unwrap();
        let path = resolve_rust_source_path("nvim-oxi", &version).unwrap();
        assert!(
            path.exists(),
            "Resolved git checkout should exist: {}",
            path.display()
        );
        assert!(path.is_dir(), "Resolved git checkout should be a directory");
    }
}
