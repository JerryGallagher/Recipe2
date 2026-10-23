use anyhow::Result;
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

/// Opens the database, creates the schema and seeds it on first run.
pub async fn open(path: &str) -> Result<Database> {
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
