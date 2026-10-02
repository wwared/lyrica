//! Resolves the frontend build directory embedded into the binary.
//!
//! `LYRICA_WEB_DIST` (set by the Nix build) wins and must exist. Otherwise
//! `web/dist` is used if built; failing that an empty directory is embedded so
//! Rust-only development works without a frontend build.

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=LYRICA_WEB_DIST");

    let dist = if let Some(dir) = env::var_os("LYRICA_WEB_DIST") {
        let dir = PathBuf::from(dir);
        assert!(
            dir.is_dir(),
            "LYRICA_WEB_DIST={} is not a directory",
            dir.display()
        );
        dir
    } else {
        let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let web_dist = manifest_dir.join("../../web/dist");
        // Rerun when the frontend gets built or rebuilt.
        println!("cargo:rerun-if-changed={}", web_dist.display());
        if web_dist.is_dir() {
            web_dist
        } else {
            println!(
                "cargo:warning=web/dist not found; embedding an empty frontend (run `npm run build` in web/)"
            );
            let empty = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("empty-web-dist");
            fs::create_dir_all(&empty).unwrap();
            empty
        }
    };

    let dist = dist.canonicalize().unwrap();
    println!("cargo:rustc-env=LYRICA_WEB_DIST={}", dist.display());
}
