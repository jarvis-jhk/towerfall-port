//! Adds the project-local SDL2 link directories (see docs/platform.md):
//! Linux: `tools/lib` (symlink to the system's libSDL2-2.0.so.0, no -dev
//! package needed); Windows (cross): the official SDL2 mingw development
//! release unpacked by `scripts/fetch-windows-toolchain.sh`, plus the
//! libgcc shims that script creates.
//! Each directory is looked up under `$TFP_ROOT` first (set by env.sh to
//! the main checkout, so git worktrees without their own `tools/` link
//! too), then relative to this checkout. `TFP_SDL2_LIB_DIR` overrides all.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=TFP_SDL2_LIB_DIR");
    println!("cargo:rerun-if-env-changed=TFP_ROOT");
    println!("cargo:rerun-if-changed=build.rs");
    if let Ok(dir) = std::env::var("TFP_SDL2_LIB_DIR") {
        println!("cargo:rustc-link-search=native={dir}");
        return;
    }
    let local = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let roots: Vec<PathBuf> = std::env::var_os("TFP_ROOT")
        .map(PathBuf::from)
        .into_iter()
        .chain(std::iter::once(local))
        .collect();
    let target = std::env::var("TARGET").unwrap_or_default();
    let dirs: &[&str] = if target.contains("windows") {
        // SDL2 import library, plus libgcc/libgcc_eh shims that map GCC's
        // runtime onto llvm-mingw's compiler-rt/libunwind.
        &[
            "tools/windows/sdl2/x86_64-w64-mingw32/lib",
            "tools/windows/gcc-shim",
        ]
    } else {
        &["tools/lib"]
    };
    for d in dirs {
        if let Some(dir) = roots.iter().map(|r| r.join(d)).find(|p| p.exists()) {
            println!(
                "cargo:rustc-link-search=native={}",
                dir.canonicalize().unwrap().display()
            );
        }
    }
}
