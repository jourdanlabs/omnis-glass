//! Governed `Page` adapter — every navigation and script eval hits TERMINUS first.

use obscura_browser::{lifecycle::WaitUntil, Page};
use serde_json::Value;

use crate::glass;
use crate::BrowserState;

pub async fn gated_navigate(
    page: &mut Page,
    url: &str,
    condition: WaitUntil,
) -> Result<(), String> {
    glass::gate_mcp_navigate(url)?;
    page.navigate_with_wait(url, condition)
        .await
        .map_err(|e| e.to_string())
}

pub fn gated_evaluate(page: &mut Page, script: &str, page_url: &str) -> Result<Value, String> {
    glass::gate_mcp_eval(script, page_url)?;
    Ok(page.evaluate(script))
}

/// Gate + eval when the caller holds `&mut BrowserState`.
pub fn gated_evaluate_state(state: &mut BrowserState, script: &str) -> Result<Value, String> {
    let page_url = state.page_url();
    gated_evaluate(state.page_mut(), script, &page_url)
}
