use std::{sync::Arc, env};

use ao3_embed::ao3::meta::{WorkMetadata, WorkTemplate};
use axum::{Router, extract::{State, Path, OriginalUri}, response::{IntoResponse, Response, Redirect, Html}, routing::get, Json, TypedHeader, headers::UserAgent, http::Uri};
use isbot::Bots;
use moka::future::Cache;
use serde::{Deserialize, Serialize};
use tower_http::normalize_path::NormalizePathLayer;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::fmt().with_file(true).init();

    let state: Arc<Cache<u64, WorkMetadata>> = Arc::new(Cache::new(100));

    let app = Router::new()
        .route("/works/:id/*path", get(work_response))
        .route("/works/:id", get(work_response))
        .route("/oembed/:id/:author/:words/:chapters/:total_chapters/:date", get(embed_response))
        .fallback(ao3_redirect)
        .layer(NormalizePathLayer::trim_trailing_slash())
        .with_state(state);

    let addr = format!("[::]:{}", env::var("PORT").unwrap_or("3000".to_owned())).parse().unwrap();

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn ao3_redirect(OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    tracing::info!("Redirecting from: {}", &uri.to_string());

    let redirect_uri = Uri::builder()
        .scheme("https")
        .authority("archiveofourown.org")
        .path_and_query(&uri.to_string())
        .build()
        .unwrap();

    Redirect::temporary(&redirect_uri.to_string())
}

#[derive(Deserialize)]
struct WorkPath {
    pub id: u64,
    pub path: Option<String>, 
}

async fn work_response(
    Path(WorkPath { id, path }): Path<WorkPath>,
    State(work_cache): State<Arc<Cache<u64, WorkMetadata>>>,
    TypedHeader(user_agent): TypedHeader<UserAgent>,
) -> Response {
    let bots = Bots::default();
    
    if !bots.is_bot(user_agent.as_str()) {
        tracing::info!("IS BOT: Redirecting");
        return Redirect::temporary(&format!("https://archiveofourown.org/works/{}/{}", id, path.unwrap_or_else(|| String::from("")))).into_response();
    }

    let work_cache = work_cache.clone();

    let Some(work) = (match work_cache.get(&id) {
        Some(work) => {
            tracing::info!("Using cached for {}", id);
            Some(work)
        },
        None => match WorkMetadata::work(id).await {
            Ok(work) => {
                work_cache.insert(id, work.clone()).await;

                tracing::info!("Caching ID: {}", id);

                Some(work)
            },
            Err(_) => None,
        }
    }) else {
        tracing::warn!("Could not retrieve meta.");
        return Redirect::temporary(&format!("https://archiveofourown.org/works/{}/{}", id, path.unwrap_or_else(|| String::from("")))).into_response();
    };

    let template: WorkTemplate = work.into();

    let Ok(html) = template.render_html() else {
        tracing::warn!("Error templating meta.");
        return Redirect::temporary(&format!("https://archiveofourown.org/works/{}/{}", id, path.unwrap_or_else(|| String::from("")))).into_response();
    };

    Html(html).into_response()
}

#[derive(Serialize)]
struct EmbedResponse {
    pub version: &'static str,
    #[serde(rename = "type")]
    pub embed_type: &'static str,
    pub author_name: String,
    pub author_url: String,
    pub provider_name: String,
    pub provider_url: String,
}

#[derive(Deserialize)]
struct EmbedRequest {
    pub id: u64,
    pub author: String,
    pub words: u64,
    pub chapters: u16,
    pub total_chapters: String,
    pub date: String,
}

async fn embed_response(
    Path(EmbedRequest { id, author, words, chapters, total_chapters, date }): Path<EmbedRequest>,
) -> Json<EmbedResponse> {
    tracing::info!("Embed Request ID: {}", id);
    Json(EmbedResponse {
        version: "1.0",
        embed_type: "rich",
        author_name: format!("{} ✏️ {} / {} 📚 {} 🕒", words, chapters, total_chapters, date),
        author_url: format!("https://archiveofourown.org/works/{}", urlencoding::encode(&id.to_string())),
        provider_name: author.clone(),
        provider_url: format!("https://archiveofourown.org/users/{}", urlencoding::encode(&author)),
    })
}
