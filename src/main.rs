mod db;

use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use tower_http::{services::ServeDir, trace::TraceLayer};
use turso::Database;

use crate::db::Recipe;

#[derive(Clone)]
struct AppState {
    db: Database,
}

/// Home page shell; htmx loads the featured recipe into it.
#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    featured_id: i64,
}

/// Full page for direct visits to /recipes/{id}.
#[derive(Template)]
#[template(path = "recipe_page.html")]
struct RecipePageTemplate {
    recipe: Recipe,
}

/// Just the recipe card, returned to htmx requests.
#[derive(Template)]
#[template(path = "recipe.html")]
struct RecipeFragmentTemplate {
    recipe: Recipe,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .init();

    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "recipes.db".into());
    let db = db::open(&db_path).await?;

    let app = router(AppState { db });

    // Cloud Run tells us which port to listen on.
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/recipes/{id}", get(show_recipe))
        .route("/healthz", get(|| async { "ok" }))
        .nest_service("/static", ServeDir::new("static"))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn index() -> Result<Html<String>, AppError> {
    Ok(Html(IndexTemplate { featured_id: 1 }.render()?))
}

async fn show_recipe(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let Some(recipe) = db::get_recipe(&state.db, id).await? else {
        return Ok((StatusCode::NOT_FOUND, "Recipe not found").into_response());
    };

    let html = if headers.contains_key("HX-Request") {
        RecipeFragmentTemplate { recipe }.render()?
    } else {
        RecipePageTemplate { recipe }.render()?
    };
    Ok(Html(html).into_response())
}

struct AppError(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!("request failed: {:#}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    async fn app() -> Router {
        let db = db::open(":memory:").await.unwrap();
        router(AppState { db })
    }

    async fn get_body(app: Router, uri: &str, htmx: bool) -> (StatusCode, String) {
        let mut req = Request::builder().uri(uri);
        if htmx {
            req = req.header("HX-Request", "true");
        }
        let res = app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
        let status = res.status();
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn full_page_includes_layout_and_recipe() {
        let (status, body) = get_body(app().await, "/recipes/1", false).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("<html"));
        assert!(body.contains("Classic Margherita Pizza"));
        assert!(body.contains("fresh mozzarella, torn"));
    }

    #[tokio::test]
    async fn htmx_request_gets_fragment_only() {
        let (status, body) = get_body(app().await, "/recipes/1", true).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.contains("<html"));
        assert!(body.contains("Classic Margherita Pizza"));
    }

    #[tokio::test]
    async fn missing_recipe_is_404() {
        let (status, _) = get_body(app().await, "/recipes/999", false).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn index_loads_recipe_with_htmx() {
        let (status, body) = get_body(app().await, "/", false).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains(r#"hx-get="/recipes/1""#));
    }
}
