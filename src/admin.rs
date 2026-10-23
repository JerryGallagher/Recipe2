//! Password-protected pages for adding recipes.

use askama::Template;
use axum::{
    Form, Router,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;

use crate::{
    AppError, AppState,
    db::{self, NewRecipe, RecipeSummary},
};

/// The admin home: the add-recipe form and the existing recipes.
#[derive(Template)]
#[template(path = "admin.html")]
struct AdminTemplate {
    form: RecipeForm,
    errors: Vec<String>,
    recipes: Vec<RecipeSummary>,
}

/// Just the form, returned to htmx when a submission has problems.
#[derive(Template)]
#[template(path = "recipe_form.html")]
struct RecipeFormTemplate {
    form: RecipeForm,
    errors: Vec<String>,
}

/// The form exactly as typed, so it can be shown again with errors.
#[derive(Default, Deserialize)]
pub struct RecipeForm {
    title: String,
    description: String,
    servings: String,
    prep_minutes: String,
    cook_minutes: String,
    ingredients: String,
    steps: String,
}

impl RecipeForm {
    fn validate(&self) -> Result<NewRecipe, Vec<String>> {
        let mut errors = Vec::new();
        let title = self.title.trim().to_string();
        if title.is_empty() {
            errors.push("Give the recipe a title.".into());
        }
        let mut number = |value: &str, label: &str, min: i64| -> i64 {
            match value.trim().parse::<i64>() {
                Ok(n) if (min..=10_000).contains(&n) => n,
                _ => {
                    errors.push(format!("{label} must be a whole number from {min} up."));
                    0
                }
            }
        };
        let servings = number(&self.servings, "Servings", 1);
        let prep_minutes = number(&self.prep_minutes, "Prep time", 0);
        let cook_minutes = number(&self.cook_minutes, "Cook time", 0);
        let ingredients = lines(&self.ingredients);
        if ingredients.is_empty() {
            errors.push("List at least one ingredient.".into());
        }
        let steps = lines(&self.steps);
        if steps.is_empty() {
            errors.push("List at least one step.".into());
        }

        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(NewRecipe {
            title,
            description: self.description.trim().to_string(),
            servings,
            prep_minutes,
            cook_minutes,
            ingredients,
            steps,
        })
    }
}

/// One entry per non-blank line, trimmed.
fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/", get(admin_home))
        .route("/recipes", post(create_recipe))
        .layer(middleware::from_fn_with_state(state, require_password))
}

/// HTTP Basic auth against `ADMIN_PASSWORD`; any username is accepted.
async fn require_password(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let Some(password) = state.admin_password.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let supplied = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|v| STANDARD.decode(v).ok())
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|v| v.split_once(':').map(|(_, p)| p.to_string()));
    match supplied {
        Some(p) if constant_time_eq(p.as_bytes(), password.as_bytes()) => next.run(req).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, r#"Basic realm="Recipe admin""#)],
            "Sign in to add recipes",
        )
            .into_response(),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn admin_home(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let recipes = db::list_recipes(&state.db).await?;
    let page = AdminTemplate {
        form: RecipeForm {
            servings: "2".into(),
            ..Default::default()
        },
        errors: Vec::new(),
        recipes,
    };
    Ok(Html(page.render()?))
}

async fn create_recipe(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<RecipeForm>,
) -> Result<Response, AppError> {
    // Browsers resend Basic auth on cross-site posts, so only accept the
    // htmx form, which sets a header another site's form cannot.
    if !headers.contains_key("HX-Request") {
        return Ok((StatusCode::FORBIDDEN, "Submit the form from /admin").into_response());
    }

    let recipe = match form.validate() {
        Ok(recipe) => recipe,
        Err(errors) => {
            // 200 so htmx swaps the form back in with the errors shown.
            return Ok(Html(RecipeFormTemplate { form, errors }.render()?).into_response());
        }
    };

    let id = {
        let _guard = state.write_lock.lock().await;
        let id = db::insert_recipe(&state.db, &recipe).await?;
        if let Some(backup) = &state.backup_path {
            db::backup(&state.db, &state.db_path, backup).await?;
        }
        id
    };
    tracing::info!(id, title = %recipe.title, "added recipe");

    let target = HeaderValue::from_str(&format!("/recipes/{id}"))?;
    Ok(([("HX-Redirect", target)], "").into_response())
}
