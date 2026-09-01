//! B1 — Mirror mode blocks navigation.

use glass_govern::{unlock_to_agent, PaneMode};
use glass_pane_bridge::{PaneSession, SessionCommand};

#[test]
fn b1_mirror_readonly_blocks_navigate() {
    unlock_to_agent();
    let mut session = PaneSession::new(true);
    assert!(matches!(session.mode(), PaneMode::Mirror));
    assert!(session.mirror_attempt_navigate().is_err());
    assert!(session
        .handle_command(SessionCommand::Navigate("https://example.com".into()))
        .is_err());
}
