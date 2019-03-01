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
use std::env;
fn set_env_with_name(name: &str) {
    let ver = match Command::new("git")
        .args(&["describe", "--tags", "--dirty=-modified"])
        .output()
    {
        Ok(cmd) => {
            if cmd.status.success() {
                    match std::str::from_utf8(&cmd.stdout[..]) {
                        Ok(v) => v.trim().to_owned(),
                        Err(_) => match env::var(name) {
                            Ok(val) => val.trim().to_owned(),
                            Err(_) => "invalid-git-version".to_owned(),
                        }
                    }
            }
            else {
                "no-git-version".to_owned()
            }
        }
        Err(_) => match env::var(name) {
            Ok(val) => val.trim().to_owned(),
            Err(_) => "no-git-version".to_owned(),
        }
    };
    println!("cargo:rustc-env={}={}", name, ver);
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    println!("cargo:rerun-if-changed=.git");
    println!("cargo:rerun-if-env-changed={}", name);
}

fn main() {
    env::set_var("RUST_BACKTRACE", "1");
    set_env_with_name("GIT_VERSION");
}
