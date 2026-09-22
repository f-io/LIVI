use std::path::Path;

fn main() {
    // include_bytes! requires the file to exist at compile time. CI drops the real .lfwb here
    // whenever its target is selected; single-target builds and local dev get an empty stub so
    // cargo check works.
    for lfwb in [
        "../../../../assets/livi-link/v821b_aic8800d80/livi-link-v821b.lfwb",
        "../../../../assets/livi-link/ax520_aic8800d80/livi-link-ax520.lfwb",
    ] {
        let path = Path::new(lfwb);
        if !path.exists() {
            let _ = std::fs::create_dir_all(path.parent().unwrap());
            let _ = std::fs::write(path, b"");
            println!(
                "cargo:warning=empty {} stub — CI populates it when its target is built",
                path.file_name().unwrap().to_string_lossy()
            );
        }
        println!("cargo:rerun-if-changed={lfwb}");
    }
}
