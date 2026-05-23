#[test]
fn manifest_has_no_signal_core_or_sema_engine_dependency() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("manifest");

    assert!(!manifest.contains("signal-core"));
    assert!(!manifest.contains("sema-engine"));
}

#[test]
fn client_does_not_access_store_or_provider_internals() {
    let client = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/client.rs"))
        .expect("client source");

    assert!(!client.contains("Store"));
    assert!(!client.contains("Cloudflare"));
    assert!(!client.contains("Google"));
    assert!(!client.contains("Hetzner"));
}
