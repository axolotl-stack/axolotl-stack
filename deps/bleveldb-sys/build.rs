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

    // This crate invokes cc directly, not CMake: no generated port_config.h
    // enables the linked codecs for us. Without these flags valid BDS SSTs
    // reach the codec's compiled-out branch and are reported as corrupt.
    build
        .define("HAVE_ZLIB", Some("1"))
        .define("HAVE_SNAPPY", Some("1"))
        .define("LEVELDB_SUPPORT_LEGACY_ZLIB_ENUM", Some("1"));
    let windows_library = if cfg!(target_os = "windows") {
        let root = find_vcpkg_root().expect("Failed to find vcpkg installation.");
        let triplet = if cfg!(target_arch = "x86_64") {
            "x64-windows-static-md"
        } else if cfg!(target_arch = "aarch64") {
            "arm64-windows-static-md"
        } else {
            panic!("Unsupported architecture for Windows platform.");
        };
        let installed = Path::new(&root).join("installed").join(triplet);
        build.include(installed.join("include"));
        Some(installed.join("lib"))
    } else {
        build.include("/usr/include");
        if cfg!(target_os = "macos") {
            build.include("/usr/local/include").include("/opt/homebrew/include");
        }
        None
    };
    println!("cargo:rerun-if-env-changed=VCPKG_ROOT");
    println!("cargo:rerun-if-env-changed=VCPKG_INSTALLATION_ROOT");

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
    if let Some(library) = windows_library {
        println!("cargo:rustc-link-search=native={}", library.display());

        println!("cargo:rustc-link-lib=static=zs");
        println!("cargo:rustc-link-lib=static=snappy");
    } else if cfg!(target_os = "linux") {
        // Add multiarch lib path (e.g. /usr/lib/x86_64-linux-gnu) where
        // static libs actually live on modern Debian/Ubuntu.
        if let Ok(output) = std::process::Command::new("dpkg-architecture")
            .arg("-qDEB_HOST_MULTIARCH")
            .output()
        {
            if let Ok(arch) = String::from_utf8(output.stdout) {
                let arch = arch.trim();
                if !arch.is_empty() {
                    println!("cargo:rustc-link-search=native=/usr/lib/{arch}");
                }
            }
        }
        println!("cargo:rustc-link-search=native=/usr/lib");
        println!("cargo:rustc-link-lib=static=z");
        println!("cargo:rustc-link-lib=pthread");
        println!("cargo:rustc-link-lib=rt");
        println!("cargo:rustc-link-lib=static=snappy");
    } else if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=static=z");
        println!("cargo:rustc-link-lib=static=snappy");
    }

}

fn find_vcpkg_root() -> Option<String> {
    for key in ["VCPKG_ROOT", "VCPKG_INSTALLATION_ROOT"] {
        if let Ok(value) = std::env::var(key)
            && !value.trim().is_empty()
        {
            return Some(value);
        }
    }

    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", "(Get-Command vcpkg).Source"])
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
