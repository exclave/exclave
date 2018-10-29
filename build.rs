use std::process::Command;

/// Same as `set_env`, but using `name` as environment variable.
///
/// You can, for example, override the `CARGO_PKG_VERSION` using in
/// your `build.rs` script:
///
/// ```
/// extern crate git_version;
/// fn main() { git_version::set_env_with_name("CARGO_PKG_VERSION"); }
/// ```
fn set_env_with_name(name: &str) {
    let cmd = Command::new("git")
        .args(&["describe", "--tags", "--dirty=-modified"])
        .output()
        .unwrap();
    assert!(cmd.status.success());
    let ver = std::str::from_utf8(&cmd.stdout[..]).unwrap().trim();
    println!("cargo:rustc-env={}={}", name, ver);
    println!("cargo:rerun-if-changed=.git/HEAD"); 
    println!("cargo:rerun-if-changed=.git/index"); 
    println!("cargo:rerun-if-changed=.git"); 
    println!("cargo:rerun-if-env-changed={}", name);
}

fn main() {
    set_env_with_name("GIT_VERSION");
}
