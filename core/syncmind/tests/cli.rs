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

    // Set XDG directories so we don't pollute the user's real config.
    let env_xdg_config = config_dir.path().to_str().unwrap();
    let env_xdg_data = data_dir.path().to_str().unwrap();

    // Register the file.
    let output = Command::new(&bin)
        .arg("register")
        .arg(&file)
        .env("XDG_CONFIG_HOME", env_xdg_config)
        .env("XDG_DATA_HOME", env_xdg_data)
        .output()
        .expect("failed to run syncmind register");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Registered") || stdout.contains("Already registered"),
        "unexpected register output: {}", stdout
    );

    // Status should show 1 registered file.
    let output = Command::new(&bin)
        .arg("status")
        .env("XDG_CONFIG_HOME", env_xdg_config)
        .env("XDG_DATA_HOME", env_xdg_data)
        .output()
        .expect("failed to run syncmind status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Registered files: 1"),
        "status should show 1 registered file: {}",
        stdout
    );

    // Unregister the file.
    let output = Command::new(&bin)
        .arg("unregister")
        .arg(&file)
        .env("XDG_CONFIG_HOME", env_xdg_config)
        .env("XDG_DATA_HOME", env_xdg_data)
        .output()
        .expect("failed to run syncmind unregister");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Unregistered") || stdout.contains("Not registered"),
        "unexpected unregister output: {}",
        stdout
    );

    // Status should show 0 registered files.
    let output = Command::new(&bin)
        .arg("status")
        .env("XDG_CONFIG_HOME", env_xdg_config)
        .env("XDG_DATA_HOME", env_xdg_data)
        .output()
        .expect("failed to run syncmind status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Registered files: 0"),
        "status should show 0 registered files: {}",
        stdout
    );
}
