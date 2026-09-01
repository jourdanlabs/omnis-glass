//! Decode CDP screencast frames.

use base64::{engine::general_purpose::STANDARD, Engine as _};

#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub png_bytes: Vec<u8>,
    pub metadata: ScreencastMetadata,
}

#[derive(Debug, Clone, Default)]
pub struct ScreencastMetadata {
    pub device_width: u32,
    pub device_height: u32,
}

pub fn decode_screencast_frame(data_b64: &str, metadata: &serde_json::Value) -> Option<DecodedFrame> {
    let png_bytes = STANDARD.decode(data_b64.trim()).ok()?;
    Some(DecodedFrame {
        png_bytes,
        metadata: ScreencastMetadata {
            device_width: metadata.get("deviceWidth").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            device_height: metadata.get("deviceHeight").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        },
    })
}
