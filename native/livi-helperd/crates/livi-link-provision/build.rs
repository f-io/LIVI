use std::path::Path;

fn main() {
    // include_bytes! requires the file to exist at compile time. CI drops the
    // real .lfwb here whenever the v821b target is selected; single-target
    // builds and local dev get an empty stub so cargo check works.
    let lfwb = Path::new("../../../../assets/livi-link/v821b_aic8800d80/livi-link-v821b.lfwb");
    if !lfwb.exists() {
        let _ = std::fs::create_dir_all(lfwb.parent().unwrap());
        let _ = std::fs::write(lfwb, b"");
        println!("cargo:warning=empty livi-link-v821b.lfwb stub — CI populates it when v821b target is built");
    }
    println!(
        "cargo:rerun-if-changed=../../../../assets/livi-link/v821b_aic8800d80/livi-link-v821b.lfwb"
    );
}
