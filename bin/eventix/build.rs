use std::process::Command;

fn main() {
    let output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse HEAD");
    let git_hash = String::from_utf8(output.stdout)
        .expect("Invalid UTF-8 in git output")
        .trim()
        .to_string();
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
}
