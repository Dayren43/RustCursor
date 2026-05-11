use std::env;
use std::path::PathBuf;

// The interception-sys crate links against interception.lib, but the matching
// interception.dll is not redistributed by the crate; it ships with the
// Interception driver release. This script copies the DLL next to the built
// executable so the binary runs without needing the DLL on PATH.
//
// Resolution order for the source DLL:
//   1. `INTERCEPTION_DLL` env var: absolute path to interception.dll
//   2. `<project_root>/vendor/interception.dll`: gitignored local copy
// If neither is found, the build still succeeds and emits a warning; the
// resulting exe will fail at startup until the DLL is placed next to it.

fn main() {
    println!("cargo:rerun-if-env-changed=INTERCEPTION_DLL");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let vendored = manifest_dir.join("vendor").join("interception.dll");

    let dll_src = match env::var("INTERCEPTION_DLL") {
        Ok(p) => PathBuf::from(p),
        Err(_) => vendored.clone(),
    };

    if !dll_src.is_file() {
        println!(
            "cargo:warning=interception.dll not found (looked at INTERCEPTION_DLL and {}). \
             Copy the DLL there or set INTERCEPTION_DLL; the exe will not start without it.",
            vendored.display()
        );
        return;
    }
    println!("cargo:rerun-if-changed={}", dll_src.display());

    // OUT_DIR is <target>/<profile>/build/<crate>-<hash>/out, so walk up 3 levels to reach <target>/<profile>.
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let Some(profile_dir) = out_dir.ancestors().nth(3) else {
        println!(
            "cargo:warning=could not derive profile dir from OUT_DIR={}",
            out_dir.display()
        );
        return;
    };

    let dll_dst = profile_dir.join("interception.dll");
    if let Err(e) = std::fs::copy(&dll_src, &dll_dst) {
        println!(
            "cargo:warning=failed to copy interception.dll to {}: {}",
            dll_dst.display(),
            e
        );
    }
}
