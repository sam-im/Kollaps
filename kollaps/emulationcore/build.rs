fn main() {
    let status = std::process::Command::new("cargo")
        .args(&["bpf", "build"])
        .current_dir("../usage.elf")
        .status()
        .expect("failed to run cargo bpf build");

    if !status.success() {
        panic!("eBPF build failed");
    }
}
