use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=../automl-web/dist");
    let profile = std::env::var("PROFILE").unwrap_or_default();
    let index = Path::new("../automl-web/dist/index.html");
    if profile == "release" && !index.exists() {
        panic!(
            "automl-web/dist/index.html missing — run \
             `npm --prefix automl-web run build` inside the AutoML workspace \
             before a release build (the SPA is embedded into the binary)."
        );
    }
    if !index.exists() {
        println!(
            "cargo:warning=automl-web/dist/ not built; SPA routes will 404 \
             until `npm --prefix automl-web run build` is run."
        );
    }
}
