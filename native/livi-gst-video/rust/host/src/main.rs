//! The livi-gst-host binary. `--probe` prints the codec support the main
//! process picks from. Otherwise it takes the socket path and where to drop a
//! crash backtrace.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--probe") {
        println!("{}", gst_video_host::gst::probe_json());
        return;
    }

    #[cfg(target_os = "linux")]
    gst_video_host::process::run(
        args.first().map_or("", String::as_str),
        args.get(1).map_or("", String::as_str),
    );

    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("livi-gst-host runs on Linux only");
        std::process::exit(1);
    }
}
