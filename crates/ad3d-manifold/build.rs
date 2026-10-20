//! Builds the vendored manifold (C++17) and its C bindings as static
//! libraries with CMake, then links them into this crate.
//!
//! - Offline: Clipper2 comes from vendor/clipper2 via
//!   FETCHCONTENT_SOURCE_DIR_CLIPPER2, and FetchContent is fully disconnected.
//! - Cross-arch: the `cmake` crate passes -DCMAKE_OSX_ARCHITECTURES=arm64 or
//!   x86_64 from TARGET, so `--target aarch64-apple-darwin` works on an Intel
//!   Mac (and the reverse). It also forwards cc's flags, so the C++ objects get
//!   the same macOS deployment target as rustc.
//! - Always CMake Release (-O3), even for `cargo build` in the dev profile:
//!   manifold at -O0 is far too slow to test with, and on MSVC this also
//!   keeps the release CRT that Rust links against.
//! - Serial: MANIFOLD_PAR=OFF, so no TBB. Parallelism is rayon's job at the
//!   AD3D level (run independent booleans on different threads).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifold_src = manifest.join("vendor").join("manifold");
    if !manifold_src.join("CMakeLists.txt").exists() || !manifest.join("vendor/clipper2/CPP/CMakeLists.txt").exists() {
        panic!(
            "vendored sources missing under {}: run crates/ad3d-manifold/fetch-vendor.sh once (downloads the pinned manifold and Clipper2 tarballs and checks their SHA-256)",
            manifest.join("vendor").display()
        );
    }
    // Clipper2's CMakeLists configure_file()s clipper.version.h into its own
    // source tree. Built in place, that touches vendor/clipper2 on every
    // configure, which re-triggers this script on every cargo build. Build a
    // synced copy in OUT_DIR instead (unchanged files keep their mtimes, so
    // nothing recompiles when the script re-runs).
    let clipper2_src = out_dir.join("clipper2-src");
    sync_dir(&manifest.join("vendor").join("clipper2"), &clipper2_src);

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=vendor/manifold");
    println!("cargo:rerun-if-changed=vendor/clipper2");
    for var in ["MACOSX_DEPLOYMENT_TARGET", "SDKROOT", "CC", "CXX", "CXXFLAGS"] {
        println!("cargo:rerun-if-env-changed={var}");
    }

    let version = manifold_version(&manifold_src.join("CMakeLists.txt"));
    println!("cargo:rustc-env=AD3D_MANIFOLD_VERSION={version}");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap();

    let mut cfg = cmake::Config::new(&manifold_src);
    cfg.profile("Release")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .define("CMAKE_INSTALL_LIBDIR", "lib")
        // The C bindings require CrossSection, which requires Clipper2.
        .define("MANIFOLD_CBIND", "ON")
        .define("MANIFOLD_CROSS_SECTION", "ON")
        .define("MANIFOLD_PAR", "OFF")
        .define("MANIFOLD_TEST", "OFF")
        .define("MANIFOLD_PYBIND", "OFF")
        .define("MANIFOLD_JSBIND", "OFF")
        .define("MANIFOLD_DEBUG", "OFF")
        // Never look for a system Clipper2/TBB and never download anything.
        .define("MANIFOLD_DOWNLOADS", "OFF")
        .define("MANIFOLD_USE_BUILTIN_CLIPPER2", "ON")
        .define("FETCHCONTENT_FULLY_DISCONNECTED", "ON")
        // FetchContent reads the upper-case name; manifold's own "may I skip
        // the download?" check (logmissingdep) reads the mixed-case one, and
        // fails the configure under MANIFOLD_DOWNLOADS=OFF without it.
        .define("FETCHCONTENT_SOURCE_DIR_CLIPPER2", &clipper2_src)
        .define("FETCHCONTENT_SOURCE_DIR_Clipper2", &clipper2_src);
    if target_env == "msvc" {
        // The cmake crate replaces CMAKE_CXX_FLAGS, dropping CMake's default /EHsc.
        cfg.cxxflag("/EHsc");
    }
    let dst = cfg.build();

    println!(
        "cargo:rustc-link-search=native={}",
        dst.join("lib").display()
    );
    // Order matters for single-pass linkers: manifoldc -> manifold -> Clipper2.
    println!("cargo:rustc-link-lib=static=manifoldc");
    println!("cargo:rustc-link-lib=static=manifold");
    println!("cargo:rustc-link-lib=static=Clipper2");

    // C++ runtime. Apple ships libc++ only as a system dylib (it cannot be
    // linked statically), so this adds no file to the .app.
    match (target_os.as_str(), target_env.as_str()) {
        ("macos" | "ios", _) => println!("cargo:rustc-link-lib=dylib=c++"),
        ("windows", "msvc") => {}
        _ => println!("cargo:rustc-link-lib=dylib=stdc++"),
    }

    // Exposed to dependents as DEP_MANIFOLD_INCLUDE (links = "manifold").
    println!(
        "cargo:include={}",
        manifold_src
            .join("bindings")
            .join("c")
            .join("include")
            .display()
    );
}

/// Copies `src` into `dst`, writing only files that are missing or differ.
fn sync_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap_or_else(|e| panic!("{}: {e}", src.display())) {
        let entry = entry.unwrap();
        let (from, to) = (entry.path(), dst.join(entry.file_name()));
        if entry.file_type().unwrap().is_dir() {
            sync_dir(&from, &to);
        } else if fs::read(&to).ok() != Some(fs::read(&from).unwrap()) {
            fs::copy(&from, &to).unwrap();
        }
    }
}

/// Reads MANIFOLD_VERSION_{MAJOR,MINOR,PATCH} from the vendored CMakeLists.txt.
fn manifold_version(cmakelists: &Path) -> String {
    let text = fs::read_to_string(cmakelists).expect("vendor/manifold/CMakeLists.txt is missing");
    let part = |name: &str| -> String {
        let key = format!("set(MANIFOLD_VERSION_{name} ");
        text.lines()
            .find_map(|l| l.trim().strip_prefix(key.as_str()))
            .and_then(|rest| rest.strip_suffix(')'))
            .map(|v| v.trim().to_string())
            .unwrap_or_else(|| panic!("MANIFOLD_VERSION_{name} not found in {}", cmakelists.display()))
    };
    format!("{}.{}.{}", part("MAJOR"), part("MINOR"), part("PATCH"))
}
