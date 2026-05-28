use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceKind {
    Turbo,
    Pnpm,
    NpmYarnBun,
}

#[derive(Debug, Clone)]
pub struct MonorepoLayout {
    pub root: PathBuf,
    pub kind: WorkspaceKind,
    pub package_rel_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct WorkspaceContext {
    pub kind: WorkspaceKind,
    pub package_rel_path: String,
    pub package_name: Option<String>,
}

pub fn detect(source_root: &Path, root_directory: Option<&Path>) -> Option<MonorepoLayout> {
    let package_dir = match root_directory {
        Some(p) if !p.as_os_str().is_empty() => source_root.join(p),
        _ => return None,
    };

    let mut cursor = package_dir.as_path();
    loop {
        if cursor == source_root {
            if let Some(kind) = detect_kind_at(cursor) {
                return finalize(source_root, cursor, &package_dir, kind);
            }
            return None;
        }

        if let Some(kind) = detect_kind_at(cursor) {
            return finalize(source_root, cursor, &package_dir, kind);
        }

        match cursor.parent() {
            Some(parent) if parent.starts_with(source_root) || parent == source_root => {
                cursor = parent;
            }
            _ => return None,
        }
    }
}

fn finalize(
    source_root: &Path,
    monorepo_root: &Path,
    package_dir: &Path,
    kind: WorkspaceKind,
) -> Option<MonorepoLayout> {
    if monorepo_root == package_dir {
        return None;
    }
    let _ = source_root;
    let rel = package_dir.strip_prefix(monorepo_root).ok()?.to_path_buf();
    Some(MonorepoLayout {
        root: monorepo_root.to_path_buf(),
        kind,
        package_rel_path: rel,
    })
}

fn detect_kind_at(dir: &Path) -> Option<WorkspaceKind> {
    if dir.join("turbo.json").exists() {
        return Some(WorkspaceKind::Turbo);
    }
    if dir.join("pnpm-workspace.yaml").exists() || dir.join("pnpm-workspace.yml").exists() {
        return Some(WorkspaceKind::Pnpm);
    }
    let pkg = dir.join("package.json");
    if pkg.exists() && has_workspaces_field(&pkg) {
        return Some(WorkspaceKind::NpmYarnBun);
    }
    None
}

fn has_workspaces_field(pkg_path: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(pkg_path) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return false;
    };
    match v.get("workspaces") {
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => o.get("packages").is_some(),
        _ => false,
    }
}

pub fn read_package_name(package_dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(package_dir.join("package.json")).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    v.get("name")
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn detects_turbo_root() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("turbo.json"), "{}").unwrap();
        let pkg = root.join("apps/web");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("package.json"), "{\"name\":\"web\"}").unwrap();

        let layout = detect(root, Some(Path::new("apps/web"))).unwrap();
        assert!(matches!(layout.kind, WorkspaceKind::Turbo));
        assert_eq!(layout.root, root);
        assert_eq!(layout.package_rel_path, PathBuf::from("apps/web"));
    }

    #[test]
    fn detects_pnpm_workspace() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("pnpm-workspace.yaml"),
            "packages:\n  - 'apps/*'\n",
        )
        .unwrap();
        let pkg = root.join("apps/api");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("package.json"), "{\"name\":\"@org/api\"}").unwrap();

        let layout = detect(root, Some(Path::new("apps/api"))).unwrap();
        assert!(matches!(layout.kind, WorkspaceKind::Pnpm));
    }

    #[test]
    fn detects_npm_workspaces_in_package_json() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("package.json"),
            r#"{"name":"root","workspaces":["packages/*"]}"#,
        )
        .unwrap();
        let pkg = root.join("packages/jobs");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("package.json"), "{\"name\":\"jobs\"}").unwrap();

        let layout = detect(root, Some(Path::new("packages/jobs"))).unwrap();
        assert!(matches!(layout.kind, WorkspaceKind::NpmYarnBun));
    }

    #[test]
    fn returns_none_when_no_markers() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let pkg = root.join("apps/web");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("package.json"), "{\"name\":\"web\"}").unwrap();

        assert!(detect(root, Some(Path::new("apps/web"))).is_none());
    }

    #[test]
    fn returns_none_when_root_directory_missing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("turbo.json"), "{}").unwrap();
        assert!(detect(root, None).is_none());
    }

    #[test]
    fn returns_none_when_root_directory_is_monorepo_root() {
        // root_directory == "" or "." means the build target IS the monorepo root —
        // treat as a single-app build, not a workspace package.
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("turbo.json"), "{}").unwrap();
        // Empty root_directory path
        assert!(detect(root, Some(Path::new(""))).is_none());
    }

    #[test]
    fn ancestor_traversal_finds_marker() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("pnpm-workspace.yaml"), "packages:\n  - '**'\n").unwrap();
        let deep = root.join("apps/web/src/feature");
        fs::create_dir_all(&deep).unwrap();
        // root_directory points deep into a subtree; ancestor traversal should find root marker.
        let layout = detect(root, Some(Path::new("apps/web"))).unwrap();
        assert_eq!(layout.root, root);
    }

    #[test]
    fn turbo_takes_priority_over_package_json_workspaces() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("turbo.json"), "{}").unwrap();
        fs::write(
            root.join("package.json"),
            r#"{"name":"root","workspaces":["apps/*"]}"#,
        )
        .unwrap();
        let pkg = root.join("apps/web");
        fs::create_dir_all(&pkg).unwrap();
        let layout = detect(root, Some(Path::new("apps/web"))).unwrap();
        assert!(matches!(layout.kind, WorkspaceKind::Turbo));
    }

    #[test]
    fn empty_workspaces_array_is_not_monorepo() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("package.json"),
            r#"{"name":"root","workspaces":[]}"#,
        )
        .unwrap();
        let pkg = root.join("apps/web");
        fs::create_dir_all(&pkg).unwrap();
        assert!(detect(root, Some(Path::new("apps/web"))).is_none());
    }

    #[test]
    fn reads_package_name() {
        let dir = tempdir().unwrap();
        let pkg = dir.path().join("apps/web");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("package.json"), r#"{"name":"@org/web"}"#).unwrap();
        assert_eq!(read_package_name(&pkg).as_deref(), Some("@org/web"));
    }
}
