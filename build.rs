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
    let cmd = match Command::new("git")
        .args(&["describe", "--tags", "--dirty=-modified"])
        .output()
    {
        Ok(rev) => rev,
        Err(e) => panic!("unable to run git to get package version: {:?}", e),
    };
    assert!(cmd.status.success());
    let ver = match std::str::from_utf8(&cmd.stdout[..]) {
        Ok(v) => v.trim(),
        Err(e) => panic!("unable to convert version number from utf8: {:?}", e),
    };
    println!("cargo:rustc-env={}={}", name, ver);
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    println!("cargo:rerun-if-changed=.git");
    println!("cargo:rerun-if-env-changed={}", name);
}

use std::env;
fn main() {
    env::set_var("RUST_BACKTRACE", "1");
    set_env_with_name("GIT_VERSION");
}
