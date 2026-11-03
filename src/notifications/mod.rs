//! Notification dispatch — outbound push plus managed listener workers for
//! configured channels.
//!
//! Direct sends flow through the driver layer, while listener-backed channels
//! (`openclaw_weixin`) uses a manager lifecycle surface
//! for startup scans, route-driven restarts, QR auto-provision, and dispatcher
//! self-heal.
//!
//! `openclaw_weixin` outbound push reads `context_token` from the cache so
//! replies actually reach the user (previously failed with `ret=-2`).
//!
//! Daily report, violation tracking, and rich (image / file) command output
//! are reserved for later phases.

pub(crate) mod channel_send;
pub(crate) mod daily_report;
mod dispatcher;
pub mod driver;
pub(crate) mod feishu_actions;
pub(crate) mod feishu_business;
pub mod feishu_events;
pub(crate) mod feishu_group_cards;
pub mod feishu_group_chats;
pub(crate) mod feishu_group_events;
pub mod feishu_group_router;
pub(crate) mod feishu_members;
pub mod feishu_native;
pub(crate) mod feishu_notify_cards;
pub(crate) mod im_commands;
pub mod manager;
pub(crate) mod openclaw_cdn;
pub(crate) mod openclaw_context_cache;
pub(crate) mod openclaw_long_poll;
pub(crate) mod openclaw_weixin;
pub(crate) mod reply_router;
pub(crate) mod sanitize;
pub mod template;
pub(crate) mod wechat_commands;

pub use dispatcher::dispatch_message_event_to_channels;
pub use dispatcher::NotificationPayload;
