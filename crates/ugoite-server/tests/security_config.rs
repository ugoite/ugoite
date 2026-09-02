#[test]
fn req_sec_001_release_compose_publishes_host_port_on_loopback() {
    let compose = include_str!("../../../docker-compose.release.yaml");
    assert!(compose.contains("127.0.0.1:${UGOITE_PORT:-8000}:8000"));
}
