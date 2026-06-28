use std::path::Path;

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("web-dist");
    if !dist.exists() {
        let _ = std::fs::create_dir_all(&dist);
    }
    println!("cargo:rerun-if-changed=web-dist");
}
