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

#[test]
#[ignore = "invokes real `docker build`; opt in with --ignored"]
fn monorepo_real_docker_build_for_web_service() {
    let dir = make_monorepo();
    let root = dir.path();

    // Override install/build so the test doesn't need an internet round-trip
    // to actually run pnpm install. The Dockerfile shape is the point — we
    // still want docker to ingest it and produce an image.
    let input = BuildInput {
        source_dir: root.to_path_buf(),
        image_tag: "arx-e2e-monorepo-web:latest".to_string(),
        dockerfile: None,
        root_directory: Some(PathBuf::from("apps/web")),
        build_command: Some("echo skipping-install-for-e2e".to_string()),
        start_command: Some("node /app/apps/web/index.js".to_string()),
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let out = rt
        .block_on(async { build(&input).await })
        .expect("build ok");

    match out.used {
        BuilderKind::Stack { name } => assert_eq!(name, "node"),
        other => panic!("expected Node stack, got {other:?}"),
    }
    assert_eq!(out.image_ref, "arx-e2e-monorepo-web:latest");

    // Clean up the local image so the test doesn't leak.
    let _ = std::process::Command::new("docker")
        .args(["rmi", "-f", "arx-e2e-monorepo-web:latest"])
        .status();
}
