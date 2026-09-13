use std::{ffi::OsString, path::PathBuf, process::Command};

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn launches_sibling_with_native_arguments_and_returns_status() {
    let directory = Fixture(std::env::temp_dir().join(format!(
        "qrate launch test {} {}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )));
    std::fs::create_dir_all(&directory.0).unwrap();
    let cli = directory
        .0
        .join(format!("qrate-cli{}", std::env::consts::EXE_SUFFIX));
    let desktop = directory
        .0
        .join(format!("qrate{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(env!("CARGO_BIN_EXE_qrate-cli"), &cli).unwrap();
    assert!(
        Command::new("rustc")
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/desktop.rs"))
            .arg("-o")
            .arg(&desktop)
            .status()
            .unwrap()
            .success()
    );
    let output = directory.0.join("arguments.txt");
    let mut names = vec![OsString::from("a project with spaces.qrate")];
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        names.push(OsString::from_vec(b"project \xff.qrate".to_vec()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let mut units: Vec<_> = "project ".encode_utf16().collect();
        units.push(0xd800);
        units.extend(".qrate".encode_utf16());
        names.push(OsString::from_wide(&units));
    }
    for name in names {
        let project = directory.0.join(name);
        std::fs::write(&project, []).unwrap();
        let status = Command::new(&cli)
            .args(["open", "--wait"])
            .arg(&project)
            .env("QRATE_TEST_OUTPUT", &output)
            .env("QRATE_TEST_EXIT", "7")
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(7));
        assert_eq!(
            std::fs::read_to_string(&output).unwrap(),
            format!("{:?}", [OsString::from("--"), project.into_os_string()])
        );
    }
    std::fs::remove_file(&output).unwrap();
    let status = Command::new(&cli)
        .env("QRATE_TEST_OUTPUT", &output)
        .env("QRATE_TEST_EXIT", "0")
        .status()
        .unwrap();
    assert!(status.success());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if std::fs::read_to_string(&output).is_ok_and(|args| args == "[]") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "desktop did not start"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
