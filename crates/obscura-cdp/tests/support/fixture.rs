//! Isolation for upstream CDP behavior tests that use local fixtures.
//! Governance and endpoint-auth tests use their own explicit configuration.

pub fn local_fixture() -> tempfile::TempDir {
    let state = tempfile::tempdir().expect("temporary fixture state");
    std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
    std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "0");
    state
}
