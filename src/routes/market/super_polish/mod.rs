mod account;
mod campaigns;
mod common;
mod compat;
mod home;
mod qualification;
mod reports;

use axum::Router;

use crate::state::AppState;

pub(crate) fn router() -> Router<AppState> {
    Router::new().nest(
        crate::sealed_protocol::sealed_str!("/super-polish"),
        routes(),
    )
}

pub(super) fn growth_alias_routes() -> Router<AppState> {
    compat::routes()
}

fn routes() -> Router<AppState> {
    Router::new()
        .merge(home::routes())
        .merge(account::routes())
        .merge(campaigns::routes())
        .merge(qualification::routes())
        .merge(reports::routes())
}
