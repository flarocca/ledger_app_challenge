use crate::middlewares::authentication::AuthContext;
use crate::state::AppState;
use axum::Extension;
use axum::extract::State;
use axum::response::Sse;
use axum::response::sse::{Event, KeepAlive};
use futures::Stream;
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

#[utoipa::path(
    get,
    path = "/feed/stream",
    tag = "feed",
    responses((status = 200, description = "Server-sent stream of new transfers", content_type = "text/event-stream"))
)]
#[tracing::instrument(skip_all)]
pub async fn stream(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthContext>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.feed.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|msg| match msg {
        Ok(ev) => Some(Ok(Event::default()
            .id(ev.id.to_string())
            .event("transfer")
            .json_data(ev)
            .unwrap_or_else(|_| Event::default()))),
        Err(_) => None,
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}
