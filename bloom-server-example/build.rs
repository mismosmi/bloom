use std::{env, fs, io::ErrorKind, path::PathBuf, process::Command};

fn wasm_pack(out_dir: &std::path::Path) -> PathBuf {
    let install_root = out_dir.join("tools");
    let installed = install_root
        .join("bin")
        .join(format!("wasm-pack{}", env::consts::EXE_SUFFIX));

    for executable in [PathBuf::from("wasm-pack"), installed.clone()] {
        match Command::new(&executable).arg("--version").output() {
            Ok(output) if output.status.success() => return executable,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            result => panic!("Could not check {}: {result:?}", executable.display()),
        }
    }

    println!("cargo:warning=wasm-pack is missing; installing it with Cargo");
    let output = Command::new(env::var_os("CARGO").expect("Cargo executable is not set"))
        .args(["install", "wasm-pack", "--locked", "--jobs", "1", "--root"])
        .arg(&install_root)
        .arg("--target-dir")
        .arg(out_dir.join("wasm-pack-target"))
        .env_remove("CARGO_BUILD_BUILD_DIR")
        .env_remove("CARGO_BUILD_DIR")
        .env_remove("CARGO_MAKEFLAGS")
        .env_remove("MAKEFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .output()
        .expect("Could not run Cargo to install wasm-pack");

    assert!(
        output.status.success(),
        "Installing wasm-pack failed ({}):\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    installed
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // wasm-pack builds this same crate's library; do not build another bundle there.
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let workspace_dir = manifest_dir.parent().unwrap();
    println!(
        "cargo:rerun-if-changed={}",
        workspace_dir.join("Cargo.toml").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        workspace_dir.join("Cargo.lock").display()
    );
    // Changes to shared components and local dependencies also affect the client bundle.
    for entry in fs::read_dir(workspace_dir).expect("Cannot read workspace directory") {
        let path = entry.expect("Cannot read workspace entry").path();
        if path.join("Cargo.toml").is_file() {
            println!(
                "cargo:rerun-if-changed={}",
                path.join("Cargo.toml").display()
            );
            if path.join("src").is_dir() {
                println!("cargo:rerun-if-changed={}", path.join("src").display());
            }
        }
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let profile = if env::var("PROFILE").as_deref() == Ok("release") {
        "--release"
    } else {
        "--dev"
    };
    let output = Command::new(wasm_pack(&out_dir))
        .current_dir(&manifest_dir)
        .args([
            "build",
            "--target",
            "web",
            "--no-typescript",
            "--no-pack",
            "--no-opt",
            profile,
        ])
        .arg("--out-dir")
        .arg(out_dir.join("pkg"))
        .args(["--", "--locked", "--jobs", "1"])
        // Nested Cargo must not wait for the parent build's directory lock or job slots.
        .env("CARGO_TARGET_DIR", out_dir.join("wasm-target"))
        .env_remove("CARGO_BUILD_BUILD_DIR")
        .env_remove("CARGO_BUILD_DIR")
        .env_remove("CARGO_MAKEFLAGS")
        .env_remove("MAKEFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .output()
        .expect("Could not run wasm-pack");

    if !output.status.success() {
        panic!(
            "Client Wasm build failed ({}):\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}
