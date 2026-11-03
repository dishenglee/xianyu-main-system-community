use axum::{routing::get, Router};

use crate::state::AppState;

pub(crate) mod error;

pub mod accounts;
pub mod assets;
pub mod automation;
pub mod business;
pub mod comments;
pub mod delivery;
pub mod health;
pub mod live;
pub mod location;
pub mod logs;
pub mod market;
pub mod notifications;
pub mod operations;
pub mod peer_monitor;
pub mod proxy;
pub mod runtime;
pub mod stats;
pub mod tasks;
pub(crate) mod tenant;

/// Public local runtime endpoints. User login and license exchange are not part
/// of this single-user, loopback-only application.
pub fn public_router() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/health"),
            get(health::health),
        )
}

/// Local business API. `app::build_app` supplies a fixed local identity and
/// binds only to loopback; the application has no product login or per-user
/// tenant model.
pub fn protected_router() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/api/health/mtop"),
            get(health::mtop_health),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/runtime"),
            get(runtime::runtime),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/api/runtime/profile"),
            get(runtime::runtime_profile),
        )
        .nest(
            crate::sealed_protocol::sealed_str!("/api"),
            assets::router(),
        )
        .nest(
            crate::sealed_protocol::sealed_str!("/api"),
            automation::router(),
        )
        .nest(
            crate::sealed_protocol::sealed_str!("/api/live"),
            live::router(),
        )
        .nest(
            crate::sealed_protocol::sealed_str!("/api"),
            accounts::router(),
        )
        .nest(
            crate::sealed_protocol::sealed_str!("/api"),
            business::router(),
        )
        .nest(
            crate::sealed_protocol::sealed_str!("/api"),
            comments::router(),
        )
        .nest(
            crate::sealed_protocol::sealed_str!("/api"),
            delivery::router(),
        )
        .nest(
            crate::sealed_protocol::sealed_str!("/api"),
            location::router(),
        )
        .nest(crate::sealed_protocol::sealed_str!("/api"), logs::router())
        .nest(
            crate::sealed_protocol::sealed_str!("/api"),
            market::router(),
        )
        .nest(
            crate::sealed_protocol::sealed_str!("/api"),
            notifications::router(),
        )
        .nest(
            crate::sealed_protocol::sealed_str!("/api"),
            operations::router(),
        )
        .nest(
            crate::sealed_protocol::sealed_str!("/api"),
            peer_monitor::router(),
        )
        .nest(crate::sealed_protocol::sealed_str!("/api"), proxy::router())
        .nest(crate::sealed_protocol::sealed_str!("/api"), stats::router())
        .nest(crate::sealed_protocol::sealed_str!("/api"), tasks::router())
}
