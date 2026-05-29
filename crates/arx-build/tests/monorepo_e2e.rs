//! End-to-end test that drives `arx_build::build()` against a real on-disk
//! monorepo and (when `docker` is available) all the way through `docker build`.
//!
//! The "shape" test runs always — it materialises a small turbo + pnpm-workspace
//! repo on disk and exercises monorepo detection + Dockerfile generation. The
//! `#[ignore]`-gated test additionally invokes `docker build`; opt in with
//! `cargo test --test monorepo_e2e -- --ignored --nocapture`.

use arx_build::{BuildInput, BuilderKind, build, monorepo};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

/// Builds an in-memory monorepo on disk:
///
/// ```
/// root/
/// ├─ turbo.json
/// ├─ pnpm-workspace.yaml
/// ├─ pnpm-lock.yaml          (empty — OK for our override flow)
/// ├─ package.json            (workspaces)
/// ├─ apps/
/// │  ├─ web/{package.json, index.js}
/// │  └─ api/{package.json, index.js}
/// └─ packages/
///    └─ shared/{package.json, index.js}
/// ```
fn make_monorepo() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("turbo.json"),
        r#"{ "$schema":"https://turbo.build/schema.json", "tasks": { "build": {}, "start": {} } }"#,
    );
    write(
        &root.join("pnpm-workspace.yaml"),
        "packages:\n  - 'apps/*'\n  - 'packages/*'\n",
    );
    write(
        &root.join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\nimporters: {}\n",
    );
    write(
        &root.join("package.json"),
        r#"{"name":"e2e-mono","private":true,"workspaces":["apps/*","packages/*"]}"#,
    );
    write(
        &root.join("apps/web/package.json"),
        r#"{"name":"web","version":"1.0.0","scripts":{"start":"node index.js","build":"node -e \"console.log('web built')\""}}"#,
    );
    write(
        &root.join("apps/web/index.js"),
        "require('http').createServer((_,r)=>r.end('web')).listen(8080)",
    );
    write(
        &root.join("apps/api/package.json"),
        r#"{"name":"api","version":"1.0.0","scripts":{"start":"node index.js","build":"node -e \"console.log('api built')\""}}"#,
    );
    write(
        &root.join("apps/api/index.js"),
        "require('http').createServer((_,r)=>r.end('api')).listen(8080)",
    );
    write(
        &root.join("packages/shared/package.json"),
        r#"{"name":"shared","version":"1.0.0"}"#,
    );
    write(&root.join("packages/shared/index.js"), "module.exports={};");

    write(&root.join(".dockerignore"), "node_modules\n.git\n");
    dir
}

#[test]
fn monorepo_shape_renders_workspace_aware_dockerfiles_for_each_service() {
    let dir = make_monorepo();
    let root = dir.path();

    // Both services should be detected as packages of the same Turbo monorepo.
    let web = monorepo::detect(root, Some(Path::new("apps/web"))).unwrap();
    let api = monorepo::detect(root, Some(Path::new("apps/api"))).unwrap();
    let shared = monorepo::detect(root, Some(Path::new("packages/shared"))).unwrap();

    assert_eq!(web.root, root);
    assert_eq!(api.root, root);
    assert_eq!(shared.root, root);
    assert!(matches!(web.kind, arx_build::WorkspaceKind::Turbo));

    // Independently verify a non-monorepo path is NOT promoted.
    let single = tempdir().unwrap();
    write(
        &single.path().join("package.json"),
        r#"{"name":"single","scripts":{"start":"node ."}}"#,
    );
    assert!(monorepo::detect(single.path(), None).is_none());
}

/// Real end-to-end build of a pnpm workspace with an actual dependency — the
/// regression test for the monorepo pnpm bug. The generated Dockerfile must
/// honor `packageManager` (pnpm@9), install once into a cached layer, and leave
/// `node_modules` populated so the build step can resolve its dependency. With
/// the old double-install bug the modules were purged and `require('is-odd')`
/// in the build script would throw, failing `docker build`.
///
/// Needs docker + network. Opt in with `--ignored`.
#[test]
#[ignore = "invokes real docker build (network); opt in with --ignored"]
fn monorepo_real_pnpm_build_resolves_dependency() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    write(
        &root.join("package.json"),
        r#"{"name":"root","private":true,"packageManager":"pnpm@9.12.0"}"#,
    );
    write(
        &root.join("pnpm-workspace.yaml"),
        "packages:\n  - 'apps/*'\n",
    );
    write(
        &root.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"is-odd":"3.0.1"},"scripts":{"build":"node -e \"require('is-odd'); console.log('ARX_E2E_DEPS_OK')\""}}"#,
    );
    write(&root.join("apps/web/index.js"), "console.log('hi');\n");

    // Produce a real frozen lockfile using pnpm inside docker (no local pnpm).
    let lockgen = std::process::Command::new("docker")
        .args(["run", "--rm", "-v"])
        .arg(format!("{}:/app", root.display()))
        .args([
            "-w",
            "/app",
            "node:22-bookworm-slim",
            "sh",
            "-lc",
            "corepack enable && corepack prepare pnpm@9.12.0 --activate && pnpm install --lockfile-only",
        ])
        .status()
        .expect("spawn docker for lockfile generation");
    assert!(lockgen.success(), "pnpm lockfile generation failed");
    assert!(
        root.join("pnpm-lock.yaml").exists(),
        "no pnpm-lock.yaml produced"
    );

    let input = BuildInput {
        source_dir: root.to_path_buf(),
        image_tag: "arx-e2e-pnpm-web:latest".to_string(),
        dockerfile: None,
        root_directory: Some(PathBuf::from("apps/web")),
        build_command: None,
        start_command: Some("node /app/apps/web/index.js".to_string()),
        build_env: vec![],
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    // A successful build proves: corepack used pnpm@9.12.0, the single frozen
    // install populated node_modules, and the dependency resolved at build time.
    let out = rt
        .block_on(async { build(&input).await })
        .expect("docker build should succeed");

    match out.used {
        BuilderKind::Stack { name } => assert_eq!(name, "node"),
        other => panic!("expected Node stack, got {other:?}"),
    }
    assert_eq!(out.image_ref, "arx-e2e-pnpm-web:latest");

    let _ = std::process::Command::new("docker")
        .args(["rmi", "-f", "arx-e2e-pnpm-web:latest"])
        .status();
}
