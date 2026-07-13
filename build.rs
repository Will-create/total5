use std::process::Command;

fn main() {
    if let Ok(output) = Command::new("rustc").arg("--version").output() {
        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout);
            println!("cargo:rustc-env=TOTAL5_RUSTC_VERSION={}", version.trim());
        }
    }
}
