// rust-embed's derive fails at COMPILE time if the embed folder is missing.
// A fresh checkout has no web/dist (build artifacts are gitignored, and vite's
// emptyOutDir would delete any placeholder), so guarantee the folder here.
fn main() {
    let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    std::fs::create_dir_all(&dist).expect("create web/dist for rust-embed");
    println!("cargo:rerun-if-changed=../../web/dist");
}
