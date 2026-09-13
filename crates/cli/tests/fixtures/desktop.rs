fn main() {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    std::fs::write(
        std::env::var_os("QRATE_TEST_OUTPUT").unwrap(),
        format!("{arguments:?}"),
    )
    .unwrap();
    std::process::exit(std::env::var("QRATE_TEST_EXIT").unwrap().parse().unwrap());
}
