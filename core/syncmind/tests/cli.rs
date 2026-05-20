use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn syncmind_bin() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("syncmind");
    path
}

#[test]
fn cli_register_unregister_status() {
    let config_dir = tempfile::tempdir().unwrap();
    let data_dir = tempfile::tempdir().unwrap();

    let file = config_dir.path().join("test.txt");
    fs::write(&file, "hello world").unwrap();

    let bin = syncmind_bin();

    // SYNCMIND_* env overrides isolate the test from the user's real
    // config/data, even on macOS where dirs::config_dir() ignores XDG_*.
    let env_config = config_dir.path().to_str().unwrap();
    let env_data = data_dir.path().to_str().unwrap();

    let run = |args: &[&str]| {
        Command::new(&bin)
            .args(args)
            .env("SYNCMIND_CONFIG_DIR", env_config)
            .env("SYNCMIND_DATA_DIR", env_data)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_DATA_HOME")
            .output()
            .unwrap_or_else(|e| panic!("failed to run syncmind {:?}: {}", args, e))
    };

    let output = run(&["register", file.to_str().unwrap()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Registered") || stdout.contains("Already registered"),
        "unexpected register output: {}", stdout
    );

    let output = run(&["status"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Registered files: 1"),
        "status should show 1 registered file: {}",
        stdout
    );

    let output = run(&["unregister", file.to_str().unwrap()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Unregistered") || stdout.contains("Not registered"),
        "unexpected unregister output: {}",
        stdout
    );

    let output = run(&["status"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Registered files: 0"),
        "status should show 0 registered files: {}",
        stdout
    );
}
