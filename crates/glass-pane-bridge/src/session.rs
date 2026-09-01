//! Pane session orchestration — mirror vs human.

use glass_govern::{
    gate_pane_navigate, lock_for_human, record_handoff_cancel, unlock_to_agent, PaneMode,
    current_mode,
};

use crate::cdp_client::CdpClient;

#[derive(Debug, Clone)]
pub enum SessionCommand {
    TakeControl,
    Release,
    Navigate(String),
}

pub struct PaneSession {
    pub mirror_only: bool,
    navigate_sent: bool,
}

impl PaneSession {
    pub fn new(mirror_only: bool) -> Self {
        Self {
            mirror_only,
            navigate_sent: false,
        }
    }

    pub fn mode(&self) -> PaneMode {
        current_mode()
    }

    pub fn take_control(&mut self) -> Option<String> {
        if self.mirror_only {
            return None;
        }
        if let Some(action) = lock_for_human() {
            record_handoff_cancel(&action);
            return Some(action);
        }
        None
    }

    pub fn release(&mut self) {
        unlock_to_agent();
    }

    /// Human navigate: gate first, then CDP. Mirror: refuse before CDP.
    pub async fn navigate_human(
        &mut self,
        client: &mut CdpClient,
        url: &str,
    ) -> Result<(), String> {
        if matches!(self.mode(), PaneMode::Mirror | PaneMode::Idle) {
            return Err("mirror_readonly: navigation blocked".into());
        }
        gate_pane_navigate(url)?;
        client.navigate(url).await?;
        self.navigate_sent = true;
        Ok(())
    }

    pub fn mirror_attempt_navigate(&self) -> Result<(), String> {
        if self.mirror_only || matches!(self.mode(), PaneMode::Mirror) {
            return Err("mirror_readonly".into());
        }
        Ok(())
    }

    pub fn did_navigate(&self) -> bool {
        self.navigate_sent
    }

    pub fn handle_command(&mut self, cmd: SessionCommand) -> Result<(), String> {
        match cmd {
            SessionCommand::TakeControl => {
                self.take_control();
                Ok(())
            }
            SessionCommand::Release => {
                self.release();
                Ok(())
            }
            SessionCommand::Navigate(url) => {
                self.mirror_attempt_navigate()?;
                gate_pane_navigate(&url)
            }
        }
    }
}
