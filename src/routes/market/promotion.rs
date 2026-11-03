use axum::Router;

use crate::state::AppState;

use super::{coin, exposure, super_polish};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .nest(
            crate::sealed_protocol::sealed_str!("/promotion"),
            promotion_routes(),
        )
        .nest(
            crate::sealed_protocol::sealed_str!("/growth"),
            promotion_routes(),
        )
}

fn promotion_routes() -> Router<AppState> {
    Router::new()
        .merge(coin::promotion_alias_routes())
        .merge(exposure::promotion_alias_routes())
        .merge(super_polish::growth_alias_routes())
}
