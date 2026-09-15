use std::process::Command;

fn main() {
    // Retrieve the current Git version for embedding into the build.
    let git_version = Command::new("git")
        .args(["describe", "--always", "--dirty", "--tags"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Set the GIT_VERSION environment variable so we can access this from the source.
    println!("cargo:rustc-env=GIT_VERSION={git_version}");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/index");
}
