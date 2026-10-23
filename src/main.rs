mod admin;
mod db;

use std::{path::PathBuf, sync::Arc};

use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use tower_http::{services::ServeDir, trace::TraceLayer};
use turso::Database;

use crate::db::{Recipe, RecipeSummary};

#[derive(Clone)]
struct AppState {
    db: Database,
    db_path: String,
    /// Where to copy the database after each change, if anywhere.
    backup_path: Option<PathBuf>,
    /// Password for /admin; the admin pages are off when this is unset.
    admin_password: Option<Arc<str>>,
    /// Serialises writes and backups.
    write_lock: Arc<tokio::sync::Mutex<()>>,
}

/// Home page shell; htmx loads the featured recipe into it.
#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    featured_id: i64,
}

/// Every recipe, by title.
#[derive(Template)]
#[template(path = "recipes.html")]
struct RecipeListTemplate {
    recipes: Vec<RecipeSummary>,
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
    let backup_path = non_empty_env("BACKUP_PATH").map(PathBuf::from);
    let admin_password = non_empty_env("ADMIN_PASSWORD").map(Arc::from);
    if admin_password.is_none() {
        tracing::warn!("ADMIN_PASSWORD is not set, so /admin is disabled");
    }
    if backup_path.is_none() {
        tracing::warn!("BACKUP_PATH is not set, so new recipes are lost on restart");
    }

    let db = db::open(&db_path, backup_path.as_deref()).await?;

    let app = router(AppState {
        db,
        db_path,
        backup_path,
        admin_password,
        write_lock: Default::default(),
    });

    // Cloud Run tells us which port to listen on.
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/recipes", get(list_recipes))
        .route("/recipes/{id}", get(show_recipe))
        .route("/healthz", get(|| async { "ok" }))
        .nest("/admin", admin::router(state.clone()))
        .nest_service("/static", ServeDir::new("static"))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn index() -> Result<Html<String>, AppError> {
    Ok(Html(IndexTemplate { featured_id: 1 }.render()?))
}

async fn list_recipes(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let recipes = db::list_recipes(&state.db).await?;
    Ok(Html(RecipeListTemplate { recipes }.render()?))
}
async fn show_recipe(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let Some(recipe) = db::get_recipe(&state.db, id).await? else {
        return Ok((StatusCode::NOT_FOUND, "Recipe not found").into_response());
    };

    // Boosted links and history restores swap in the whole page, so only a
    // targeted htmx request (the home page's loader) gets the bare card.
    let fragment = headers.contains_key("HX-Request")
        && !headers.contains_key("HX-Boosted")
        && !headers.contains_key("HX-History-Restore-Request");
    let html = if fragment {
        RecipeFragmentTemplate { recipe }.render()?
    } else {
        RecipePageTemplate { recipe }.render()?
    };
    // The same URL returns two shapes, so caches must key on these headers.
    let vary = [(
        header::VARY,
        "HX-Request, HX-Boosted, HX-History-Restore-Request",
    )];
    Ok((vary, Html(html)).into_response())
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

    fn state(db: Database, db_path: &str, backup_path: Option<PathBuf>) -> AppState {
        AppState {
            db,
            db_path: db_path.into(),
            backup_path,
            admin_password: Some("secret".into()),
            write_lock: Default::default(),
        }
    }

    async fn app() -> Router {
        let db = db::open(":memory:", None).await.unwrap();
        router(state(db, ":memory:", None))
    }

    fn basic(password: &str) -> String {
        use base64::Engine;
        let creds = base64::engine::general_purpose::STANDARD.encode(format!("admin:{password}"));
        format!("Basic {creds}")
    }

    const GOOD_FORM: &str = "title=Toast&description=Crunchy&servings=1&prep_minutes=1\
        &cook_minutes=3&ingredients=1+slice+bread%0D%0A%0D%0AButter&steps=Toast+it.%0ASpread+butter.";

    async fn post_recipe(app: Router, auth: Option<&str>, htmx: bool, form: &str) -> Response {
        let mut req = Request::builder()
            .method("POST")
            .uri("/admin/recipes")
            .header("Content-Type", "application/x-www-form-urlencoded");
        if let Some(auth) = auth {
            req = req.header("Authorization", auth);
        }
        if htmx {
            req = req.header("HX-Request", "true");
        }
        app.oneshot(req.body(Body::from(form.to_string())).unwrap())
            .await
            .unwrap()
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
    async fn boosted_link_gets_full_page() {
        let req = Request::builder()
            .uri("/recipes/1")
            .header("HX-Request", "true")
            .header("HX-Boosted", "true")
            .body(Body::empty())
            .unwrap();
        let res = app().await.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            res.headers()["Vary"]
                .to_str()
                .unwrap()
                .contains("HX-Request")
        );
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("site-header"));
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

    #[tokio::test]
    async fn recipe_list_shows_titles() {
        let (status, body) = get_body(app().await, "/recipes", false).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains(r#"href="/recipes/1""#));
    }

    #[tokio::test]
    async fn admin_needs_password() {
        let (status, _) = get_body(app().await, "/admin", false).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let req = Request::builder()
            .uri("/admin")
            .header("Authorization", basic("wrong"))
            .body(Body::empty())
            .unwrap();
        let res = app().await.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        let req = Request::builder()
            .uri("/admin")
            .header("Authorization", basic("secret"))
            .body(Body::empty())
            .unwrap();
        let res = app().await.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn admin_is_off_without_password() {
        let db = db::open(":memory:", None).await.unwrap();
        let mut st = state(db, ":memory:", None);
        st.admin_password = None;
        let req = Request::builder()
            .uri("/admin")
            .header("Authorization", basic("secret"))
            .body(Body::empty())
            .unwrap();
        let res = router(st).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn adding_a_recipe_publishes_it() {
        let app = app().await;
        let res = post_recipe(app.clone(), Some(&basic("secret")), true, GOOD_FORM).await;
        assert_eq!(res.status(), StatusCode::OK);
        let target = res.headers()["HX-Redirect"].to_str().unwrap().to_string();
        assert_eq!(target, "/recipes/2");

        let (status, body) = get_body(app, &target, false).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("Toast"));
        assert!(body.contains("<li>1 slice bread</li>"));
        assert!(body.contains("<li>Butter</li>"));
        assert!(body.contains("<li>Spread butter.</li>"));
    }

    #[tokio::test]
    async fn invalid_recipe_shows_errors() {
        let form =
            "title=&description=&servings=0&prep_minutes=x&cook_minutes=1&ingredients=&steps=";
        let res = post_recipe(app().await, Some(&basic("secret")), true, form).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(!res.headers().contains_key("HX-Redirect"));
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("Give the recipe a title."));
        assert!(body.contains("Servings must be"));
        assert!(body.contains("Prep time must be"));
        assert!(body.contains("List at least one step."));
    }

    #[tokio::test]
    async fn posts_without_htmx_are_refused() {
        let res = post_recipe(app().await, Some(&basic("secret")), false, GOOD_FORM).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let res = post_recipe(app().await, None, true, GOOD_FORM).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn recipes_survive_a_restart_via_backup() {
        let dir = std::env::temp_dir().join(format!("recipe-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bucket")).unwrap();
        let db_path = dir.join("local.db").to_str().unwrap().to_string();
        let backup = dir.join("bucket/recipes.db");

        let db = db::open(&db_path, Some(&backup)).await.unwrap();
        let app = router(state(db, &db_path, Some(backup.clone())));
        let res = post_recipe(app, Some(&basic("secret")), true, GOOD_FORM).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(backup.exists());

        // A fresh container: no local database, only the backup.
        let db_path2 = dir.join("fresh.db").to_str().unwrap().to_string();
        let db = db::open(&db_path2, Some(&backup)).await.unwrap();
        let recipes = db::list_recipes(&db).await.unwrap();
        let titles: Vec<_> = recipes.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["Classic Margherita Pizza", "Toast"]);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
