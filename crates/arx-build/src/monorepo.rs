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
    /// Root-relative paths of every workspace package's `package.json`, used to
    /// COPY just the manifests into a cached dependency-install layer. Empty
    /// when enumeration failed (caller falls back to copying the whole tree).
    pub workspace_manifests: Vec<String>,
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

/// Root-relative paths (`<dir>/package.json`) of every workspace package, by
/// expanding the workspace globs against the on-disk monorepo `root`. Returns
/// empty when the globs can't be read/parsed — callers fall back to copying the
/// whole tree before install, so a partial set never breaks `--frozen-lockfile`.
pub fn workspace_manifest_paths(root: &Path) -> Vec<String> {
    let patterns = workspace_globs(root);
    if patterns.is_empty() {
        return Vec::new();
    }
    let mut builder = globset::GlobSetBuilder::new();
    let mut any = false;
    for p in &patterns {
        // `literal_separator(true)` makes `*` a single path segment (fast-glob
        // semantics used by pnpm/npm), so `apps/*` matches `apps/web` only.
        if let Ok(g) = globset::GlobBuilder::new(p).literal_separator(true).build() {
            builder.add(g);
            any = true;
        }
    }
    if !any {
        return Vec::new();
    }
    let Ok(set) = builder.build() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_package_dirs(root, root, &set, 0, &mut out);
    out.sort();
    out.dedup();
    out
}

fn workspace_globs(root: &Path) -> Vec<String> {
    for f in ["pnpm-workspace.yaml", "pnpm-workspace.yml"] {
        if let Ok(raw) = std::fs::read_to_string(root.join(f)) {
            let pats = parse_pnpm_workspace_packages(&raw);
            if !pats.is_empty() {
                return pats;
            }
        }
    }
    if let Ok(raw) = std::fs::read_to_string(root.join("package.json"))
        && let Ok(v) = serde_json::from_str::<Value>(&raw)
    {
        return package_json_workspace_globs(&v);
    }
    Vec::new()
}

fn package_json_workspace_globs(v: &Value) -> Vec<String> {
    let arr = match v.get("workspaces") {
        Some(Value::Array(a)) => a,
        Some(Value::Object(o)) => match o.get("packages") {
            Some(Value::Array(a)) => a,
            _ => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    arr.iter()
        .filter_map(|x| x.as_str())
        .filter(|s| !s.starts_with('!'))
        .map(|s| s.to_string())
        .collect()
}

/// Minimal line-based reader for the `packages:` list in `pnpm-workspace.yaml`
/// (avoids pulling in a YAML dependency). Negation patterns (`!...`) are skipped.
fn parse_pnpm_workspace_packages(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_packages = false;
    for line in raw.lines() {
        if !in_packages {
            if line.trim_end() == "packages:" {
                in_packages = true;
            }
            continue;
        }
        // A non-indented, non-empty line starts the next top-level key.
        if !line.starts_with([' ', '\t']) && !line.trim().is_empty() {
            break;
        }
        if let Some(rest) = line.trim_start().strip_prefix('-') {
            let item = strip_yaml_quotes(rest.trim());
            if !item.is_empty() && !item.starts_with('!') {
                out.push(item.to_string());
            }
        }
    }
    out
}

fn strip_yaml_quotes(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2
        && ((s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn collect_package_dirs(
    root: &Path,
    dir: &Path,
    set: &globset::GlobSet,
    depth: usize,
    out: &mut Vec<String>,
) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || matches!(name.as_ref(), "node_modules" | "target") {
            continue;
        }
        if let Ok(rel) = path.strip_prefix(root) {
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if set.is_match(&rel_str) && path.join("package.json").is_file() {
                out.push(format!("{rel_str}/package.json"));
            }
        }
        collect_package_dirs(root, &path, set, depth + 1, out);
    }
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

    #[test]
    fn manifest_paths_from_pnpm_workspace() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("pnpm-workspace.yaml"),
            "packages:\n  - 'apps/*'\n  - 'packages/*'\n",
        )
        .unwrap();
        for p in ["apps/web", "apps/api", "packages/ui"] {
            let d = root.join(p);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("package.json"), "{}").unwrap();
        }
        // a nested dir without package.json must not be picked up
        fs::create_dir_all(root.join("apps/web/src")).unwrap();

        let mut got = workspace_manifest_paths(root);
        got.sort();
        assert_eq!(
            got,
            vec![
                "apps/api/package.json".to_string(),
                "apps/web/package.json".to_string(),
                "packages/ui/package.json".to_string(),
            ]
        );
    }

    #[test]
    fn manifest_paths_from_package_json_workspaces() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("package.json"),
            r#"{"name":"root","workspaces":["packages/*"]}"#,
        )
        .unwrap();
        let d = root.join("packages/jobs");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("package.json"), "{}").unwrap();

        assert_eq!(
            workspace_manifest_paths(root),
            vec!["packages/jobs/package.json".to_string()]
        );
    }

    #[test]
    fn manifest_paths_empty_when_no_definition() {
        let dir = tempdir().unwrap();
        assert!(workspace_manifest_paths(dir.path()).is_empty());
    }

    #[test]
    fn star_does_not_cross_path_separator() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("pnpm-workspace.yaml"),
            "packages:\n  - 'apps/*'\n",
        )
        .unwrap();
        // apps/web is a package; apps/web/nested is NOT matched by `apps/*`
        let nested = root.join("apps/web/nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("apps/web/package.json"), "{}").unwrap();
        fs::write(nested.join("package.json"), "{}").unwrap();

        assert_eq!(
            workspace_manifest_paths(root),
            vec!["apps/web/package.json".to_string()]
        );
    }
}
