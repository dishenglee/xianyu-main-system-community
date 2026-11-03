//! 闲鱼 IM WS 长连接同步 + live 事件解析 (split from accounts/mod.rs in B4.5).
//! Refactored into sub-modules in B4.6:
//!   - event_parser:    frame parsing + ACCS sync payload decoding
//!   - event_processor: hydration, persistence, and pub(crate) dispatch
//!   - ws_sync:         short-lived WS connections for sync/history

mod event_parser;
mod event_processor;
mod ws_sync;

// Re-export pub(crate) items that other crate modules depend on.
pub(crate) use event_parser::parse_ws_json_frame;
pub(crate) use event_processor::{
    decode_and_process_live_sync_payload, process_live_message_event,
    try_process_fallback_live_event,
};

// Re-export items consumed by accounts/mod.rs and messages/history.rs.
pub(crate) use ws_sync::{
    fetch_message_history_via_active_socket, fetch_message_history_via_ws,
    sync_recent_message_events_via_ws,
};

// Re-export for normalize module (messages/normalize/mod.rs uses super::super::live::).
pub(crate) use event_parser::try_parse_date_format_timestamp;
