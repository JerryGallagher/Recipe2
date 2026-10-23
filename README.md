# Recipe Website

A small recipe site written in Rust.

- **Web:** [axum](https://github.com/tokio-rs/axum) with [askama](https://github.com/askama-rs/askama) templates
- **Frontend:** server-rendered HTML with [htmx](https://htmx.org) (vendored in `static/`)
- **Database:** [Turso](https://github.com/tursodatabase/turso) (formerly Limbo), the SQLite-compatible database written in Rust
- **Hosting:** Google Cloud Run, built from the `Dockerfile`

## Running locally

```sh
cargo run
# open http://localhost:8080
```

| Variable        | Default      | Purpose                        |
|-----------------|--------------|--------------------------------|
| `PORT`          | `8080`       | Port to listen on (Cloud Run sets this) |
| `DATABASE_PATH` | `recipes.db` | Database file; `:memory:` works too |

On startup the app creates the schema (`migrations/001_schema.sql`) and, if the
database is empty, loads the seed data (`migrations/002_seed.sql`).

## Routes

- `GET /` loads the featured recipe into the page with htmx
- `GET /recipes/{id}` returns the full page, or just the recipe card when the
  request comes from htmx (`HX-Request` header)
- `GET /healthz` returns `ok`

## Tests

```sh
cargo test
```

## Deploying to Cloud Run

The container keeps its database in `/tmp`, which Cloud Run wipes whenever an
instance restarts, and the seed data is reloaded each time. That is fine while
the recipes live in the seed file; persistent storage comes later.

From a machine with `gcloud` signed in:

```sh
gcloud run deploy recipe-website --source . --region us-central1 --allow-unauthenticated
```

Or connect the GitHub repo in the Cloud Run console ("Continuously deploy from a
repository", build type Dockerfile) so every push to `main` deploys.
