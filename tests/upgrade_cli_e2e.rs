use std::process::Command;

fn run_gargo(args: &[&str], state: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_gargo"))
        .args(args)
        .env("GARGO_TEST_UPDATE_SOURCE", "mock")
        .env("GARGO_TEST_UPDATE_STATE", state)
        .output()
        .expect("run gargo binary")
}

#[test]
fn check_reports_up_to_date_in_mock_mode() {
    let output = run_gargo(&["--check"], "up_to_date");
    assert!(
        output.status.success(),
        "expected success for --check up_to_date: {:?}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Already up to date"),
        "unexpected stdout: {stdout}"
    );
}

#[test]
fn check_reports_available_update_in_mock_mode() {
    let output = run_gargo(&["--check"], "has_update");
    assert!(
        output.status.success(),
        "expected success for --check has_update: {:?}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Update available"),
        "unexpected stdout: {stdout}"
    );
}

#[test]
fn upgrade_reports_success_in_mock_mode() {
    let output = run_gargo(&["--upgrade"], "has_update");
    assert!(
        output.status.success(),
        "expected success for --upgrade has_update: {:?}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Upgraded gargo from"),
        "unexpected stdout: {stdout}"
    );
}

#[test]
fn upgrade_reports_failure_in_mock_mode() {
    let output = run_gargo(&["--upgrade"], "error");
    assert!(
        !output.status.success(),
        "expected failure for --upgrade error: {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Error:"),
        "unexpected stderr prefix for upgrade error: {stderr}"
    );
    assert!(
        stderr.contains("mock"),
        "unexpected stderr detail for upgrade error: {stderr}"
    );
}
