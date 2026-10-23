use std::path::Path;

use anyhow::{Context, Result};
use turso::{Builder, Database};

const SCHEMA: &str = include_str!("../migrations/001_schema.sql");
const SEED: &str = include_str!("../migrations/002_seed.sql");

pub struct Recipe {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub servings: i64,
    pub prep_minutes: i64,
    pub cook_minutes: i64,
    pub ingredients: Vec<String>,
    pub steps: Vec<String>,
}

impl Recipe {
    pub fn total_minutes(&self) -> i64 {
        self.prep_minutes + self.cook_minutes
    }
}

/// A recipe submitted from the admin form, before it has an id.
pub struct NewRecipe {
    pub title: String,
    pub description: String,
    pub servings: i64,
    pub prep_minutes: i64,
    pub cook_minutes: i64,
    pub ingredients: Vec<String>,
    pub steps: Vec<String>,
}

/// One row of the recipe list.
pub struct RecipeSummary {
    pub id: i64,
    pub title: String,
    pub description: String,
}

/// Opens the database, creates the schema and seeds it on first run.
///
/// With a `backup_path`, a copy saved there by an earlier run is restored
/// first, so recipes survive container restarts.
pub async fn open(path: &str, backup_path: Option<&Path>) -> Result<Database> {
    if let Some(backup) = backup_path {
        restore(path, backup)?;
    }
    let db = Builder::new_local(path).build().await?;
    let conn = db.connect()?;
    conn.execute_batch(SCHEMA).await?;

    let mut rows = conn.query("SELECT COUNT(*) FROM recipes", ()).await?;
    let count: i64 = match rows.next().await? {
        Some(row) => row.get(0)?,
        None => 0,
    };
    drop(rows);
    if count == 0 {
        conn.execute_batch(SEED).await?;
        tracing::info!("seeded database");
    }
    Ok(db)
}

pub async fn get_recipe(db: &Database, id: i64) -> Result<Option<Recipe>> {
    let conn = db.connect()?;

    let mut rows = conn
        .query(
            "SELECT id, title, description, servings, prep_minutes, cook_minutes \
             FROM recipes WHERE id = ?1",
            (id,),
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(None);
    };
    let mut recipe = Recipe {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        servings: row.get(3)?,
        prep_minutes: row.get(4)?,
        cook_minutes: row.get(5)?,
        ingredients: Vec::new(),
        steps: Vec::new(),
    };
    drop(rows);

    recipe.ingredients = list_text(
        &conn,
        "SELECT text FROM ingredients WHERE recipe_id = ?1 ORDER BY position",
        id,
    )
    .await?;
    recipe.steps = list_text(
        &conn,
        "SELECT text FROM steps WHERE recipe_id = ?1 ORDER BY position",
        id,
    )
    .await?;
    Ok(Some(recipe))
}

async fn list_text(conn: &turso::Connection, sql: &str, id: i64) -> Result<Vec<String>> {
    let mut rows = conn.query(sql, (id,)).await?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        out.push(row.get(0)?);
    }
    Ok(out)
}

pub async fn list_recipes(db: &Database) -> Result<Vec<RecipeSummary>> {
    let conn = db.connect()?;
    let mut rows = conn
        .query(
            "SELECT id, title, description FROM recipes ORDER BY title",
            (),
        )
        .await?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        out.push(RecipeSummary {
            id: row.get(0)?,
            title: row.get(1)?,
            description: row.get(2)?,
        });
    }
    Ok(out)
}

/// Inserts a recipe with its ingredients and steps, returning the new id.
pub async fn insert_recipe(db: &Database, recipe: &NewRecipe) -> Result<i64> {
    let mut conn = db.connect()?;
    let tx = conn.transaction().await?;
    tx.execute(
        "INSERT INTO recipes (title, description, servings, prep_minutes, cook_minutes) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        (
            recipe.title.as_str(),
            recipe.description.as_str(),
            recipe.servings,
            recipe.prep_minutes,
            recipe.cook_minutes,
        ),
    )
    .await?;
    let id = tx.last_insert_rowid();
    for (i, text) in recipe.ingredients.iter().enumerate() {
        tx.execute(
            "INSERT INTO ingredients (recipe_id, position, text) VALUES (?1, ?2, ?3)",
            (id, i as i64 + 1, text.as_str()),
        )
        .await?;
    }
    for (i, text) in recipe.steps.iter().enumerate() {
        tx.execute(
            "INSERT INTO steps (recipe_id, position, text) VALUES (?1, ?2, ?3)",
            (id, i as i64 + 1, text.as_str()),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(id)
}

/// Copies the database to `backup` so it outlives this container.
///
/// The WAL is folded into the main file first so a single file holds
/// everything, and the copy is written beside the target then renamed so a
/// crash never leaves a half-written backup.
pub async fn backup(db: &Database, path: &str, backup: &Path) -> Result<()> {
    let conn = db.connect()?;
    let mut rows = conn.query("PRAGMA wal_checkpoint(TRUNCATE)", ()).await?;
    while rows.next().await?.is_some() {}
    drop(rows);

    let tmp = backup.with_extension("tmp");
    std::fs::copy(path, &tmp).with_context(|| format!("copying database to {}", tmp.display()))?;
    std::fs::rename(&tmp, backup)?;
    Ok(())
}

/// Restores `backup` to `path` when there is a backup and no local database.
fn restore(path: &str, backup: &Path) -> Result<()> {
    if !backup.exists() || Path::new(path).exists() {
        return Ok(());
    }
    std::fs::copy(backup, path)
        .with_context(|| format!("restoring database from {}", backup.display()))?;
    tracing::info!("restored database from {}", backup.display());
    Ok(())
}
