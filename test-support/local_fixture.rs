// Used only by test processes that exercise local network fixtures.
pub struct FixtureState(std::path::PathBuf);
impl Drop for FixtureState {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}
pub fn isolated_state() -> FixtureState {
    let path = std::env::temp_dir().join(format!("glass-fixture-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&path).unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", &path);
    FixtureState(path)
}
pub fn local_fixture() -> FixtureState {
    let state = isolated_state();
    std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
    std::env::set_var("OMNIS_GLASS_LANE", "browse:local");
    std::env::remove_var("OMNIS_GLASS_LANES");
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    state
}
