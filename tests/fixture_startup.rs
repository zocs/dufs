mod fixtures;

use fixtures::{port, server, server_on_port};
use std::net::TcpListener;

#[test]
fn occupied_candidate_port_is_retried_without_attaching_to_another_listener() {
    let candidate = port();
    let listener = TcpListener::bind(format!("127.0.0.1:{candidate}")).unwrap();
    let occupied = listener.local_addr().unwrap().port();
    // Require the exact occupied address. Wildcard/default binds can coexist
    // with a specific-address listener under platform-specific socket rules.
    let server = server_on_port(&["--bind", "127.0.0.1"], candidate);
    assert_ne!(server.port(), occupied);
    let response = reqwest::blocking::get(server.url().join("test.txt").unwrap()).unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().unwrap(), "This is test.txt");
}

#[test]
fn failed_child_reports_its_exit_and_diagnostic_instead_of_a_socket_timeout() {
    let failure = std::panic::catch_unwind(|| server(&["--invalid-fixture-option"]));
    let panic = failure.err().expect("invalid arguments must fail startup");
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .expect("startup failure should include a diagnostic string");
    assert!(message.contains("exited"), "{message}");
    assert!(message.contains("invalid-fixture-option"), "{message}");
}

#[test]
fn ready_fixture_serves_its_own_files() {
    let server = server(&[] as &[&str]);
    assert_eq!(
        reqwest::blocking::get(server.url().join("test.txt").unwrap())
            .unwrap()
            .text()
            .unwrap(),
        "This is test.txt"
    );
}
