// RDP client session management.
//
// This module will be fully implemented in Phase 5.
// Uses IronRDP for protocol handling and frame decoding.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// Decoded frame data to send to the frontend Canvas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpFrame {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// RGBA pixel data for this frame region.
    pub data: Vec<u8>,
}

/// Manages an active RDP session.
pub struct RdpSession {
    // Will hold IronRDP session and frame sender
}

impl RdpSession {
    /// Establish an RDP connection and start frame decoding.
    pub async fn connect(
        _host: &str,
        _port: u16,
        _username: &str,
        _password: &str,
        _width: u32,
        _height: u32,
        _frame_tx: mpsc::UnboundedSender<RdpFrame>,
    ) -> Result<Self> {
        todo!("RDP connection not yet implemented")
    }

    /// Send a keyboard event to the remote desktop.
    pub async fn send_key_event(&self, _key_code: u16, _pressed: bool) -> Result<()> {
        todo!("RDP key event not yet implemented")
    }

    /// Send a mouse event to the remote desktop.
    pub async fn send_mouse_event(
        &self,
        _x: u32,
        _y: u32,
        _buttons: u8,
        _wheel: i8,
    ) -> Result<()> {
        todo!("RDP mouse event not yet implemented")
    }

    /// Gracefully disconnect the RDP session.
    pub async fn disconnect(self) -> Result<()> {
        todo!("RDP disconnect not yet implemented")
    }
}
