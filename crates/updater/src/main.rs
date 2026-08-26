use std::path::PathBuf;

fn main() {
    let job = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| {
            updater::updates_dir()
                .ok()
                .map(|dir| dir.join(updater::JOB_NAME))
        })
        .unwrap_or_else(|| {
            eprintln!("could not locate qrate's pending update job");
            std::process::exit(2);
        });
    if let Err(error) = updater::run_job(&job) {
        eprintln!("qrate update failed: {error:#}");
        std::process::exit(1);
    }
}
