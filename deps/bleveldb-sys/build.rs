use std::{path::Path, process::Command};

fn main() {
    let base = Path::new("leveldb");

    // Include paths for LevelDB headers
    let mut build = cc::Build::new();

    build
        .cpp(true)
        .flag_if_supported("-std=c++17")
        .warnings(false)
        .include(base.join("include"))
        .include(base.join("port"))
        .include(base.to_path_buf());

    // Compile LevelDB source files
    for dir in ["db", "table", "util", "port"] {
        let pattern = base.join(dir).join("*.cc");
        for entry in glob::glob(pattern.to_str().unwrap()).unwrap() {
            let path = entry.unwrap();
            let file_name = path.file_name().unwrap().to_string_lossy();

            // Skip certain files not needed for the build
            // Skip test files
            if file_name.contains("test") || file_name.contains("bench") {
                continue;
            }

            if cfg!(target_os = "windows") {
                // Skip files that are not compatible with Windows
                if file_name == "env_posix.cc" {
                    continue;
                }
            }

            if cfg!(target_os = "linux") {
                // Skip Windows-specific files on non-Windows platforms
                if file_name == "env_windows.cc" {
                    continue;
                }
            }

            build.file(&path);
        }
    }

    if cfg!(target_os = "windows") {
        build.define("LEVELDB_PLATFORM_WINDOWS", None);
        build.define("NOMINMAX", None);
    } else if cfg!(target_os = "linux") {
        build.define("LEVELDB_PLATFORM_POSIX", None);
    } else if cfg!(target_os = "macos") {
        build.define("LEVELDB_PLATFORM_POSIX", None);
        build.flag_if_supported("-stdlib=libc++");
    }

    // Build as a static library
    build.compile("leveldb");
    println!("cargo:rerun-if-changed=leveldb/");

    // Link libraries needed by LevelDB
    if cfg!(target_os = "windows") {
        let vcpkg_path = find_vcpkg_root().expect("Failed to find vcpkg installation.");
        let vcpkg_root = Path::new(&vcpkg_path);

        if cfg!(target_arch = "x86") {
            panic!("32-bit architecture is not supported. Please use x64 architecture.");
        }

        let mut triplet = "";

        // updated to static-md so that we dont have to provide dlls to users.
        if cfg!(target_arch = "x86_64") {
            triplet = "x64-windows-static-md";
        } else if cfg!(target_arch = "aarch64") {
            triplet = "arm64-windows-static-md";
        } else {
            let _ = triplet;
            panic!("Unsupported architecture for Windows platform.");
        }

        println!(
            "cargo:rustc-link-search=native={}",
            vcpkg_root
                .join("installed")
                .join(triplet)
                .join("lib")
                .display()
        );

        println!("cargo:rustc-link-lib=static=zlib");
        println!("cargo:rustc-link-lib=static=snappy");
    } else if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-search=native={}", "/usr/lib");
        println!("cargo:rustc-link-lib=static=z");
        println!("cargo:rustc-link-lib=pthread");
        println!("cargo:rustc-link-lib=rt");
        println!("cargo:rustc-link-lib=static=snappy");
    } else if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=static=z");
        println!("cargo:rustc-link-lib=static=snappy");
    }

    // Optional: print current submodule commit hash for debugging
    if let Ok(output) = std::process::Command::new("git")
        .args(&["-C", "leveldb", "rev-parse", "--short", "HEAD"])
        .output()
    {
        if let Ok(hash) = String::from_utf8(output.stdout) {
            println!("cargo:warning=Building LevelDB commit: {}", hash.trim());
        }
    }
}

fn find_vcpkg_root() -> Option<String> {
    let output = Command::new("powershell")
        .args(&["/C", "(Get-Command vcpkg).Source"])
        .output()
        .ok()?;

    let vcpkg_path = String::from_utf8(output.stdout).ok()?;
    let vcpkg_root = Path::new(&vcpkg_path)
        .parent()
        .expect("Failed to determine vcpkg root directory")
        .as_os_str()
        .to_string_lossy();

    Some(vcpkg_root.to_string())
}
