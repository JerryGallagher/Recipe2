# Recipe Website

A small recipe site written in Rust.

- **Web:** [axum](https://github.com/tokio-rs/axum) with [askama](https://github.com/askama-rs/askama) templates
- **Frontend:** server-rendered HTML with [htmx](https://htmx.org) (vendored in `static/`)
- **Database:** [Turso](https://github.com/tursodatabase/turso) (formerly Limbo), the SQLite-compatible database written in Rust
- **Hosting:** Google Cloud Run, built from the `Dockerfile`

## Running locally

```sh
ADMIN_PASSWORD=secret cargo run
# open http://localhost:8080, or http://localhost:8080/admin to add recipes
```

| Variable         | Default      | Purpose                        |
|------------------|--------------|--------------------------------|
| `PORT`           | `8080`       | Port to listen on (Cloud Run sets this) |
| `DATABASE_PATH`  | `recipes.db` | Database file; `:memory:` works too |
| `ADMIN_PASSWORD` | unset        | Password for `/admin`; the admin pages are off when unset |
| `BACKUP_PATH`    | unset        | Where to copy the database after each change, e.g. a Cloud Storage mount |

On startup the app creates the schema (`migrations/001_schema.sql`) and, if the
database is empty, loads the seed data (`migrations/002_seed.sql`).

## Routes

- `GET /` loads the featured recipe into the page with htmx
- `GET /recipes` lists every recipe
- `GET /recipes/{id}` returns the full page, or just the recipe card when the
  request comes from htmx (`HX-Request` header)
- `GET /admin` (password protected) has a form for adding recipes, which
  posts to `POST /admin/recipes` with htmx
- `GET /healthz` returns `ok`

## Admin

Visit `/admin` and the browser asks for a username and password; the username
can be anything and the password is `ADMIN_PASSWORD`. Browsers resend these
credentials on requests from other sites too, so `POST /admin/recipes` only
accepts requests carrying htmx's `HX-Request` header, which another site's
form cannot set.

## Keeping recipes across restarts

The working database is a local file, which on Cloud Run lives in memory and
vanishes when an instance stops. With `BACKUP_PATH` set, the app copies the
whole database there after every change and, when it starts without a local
database, restores from that copy first. On Cloud Run, `BACKUP_PATH` points
into a Cloud Storage bucket mounted as a volume.

This assumes one instance writes at a time, so the service runs with
`--max-instances=1`.

## Tests

```sh
cargo test
```

## Deploying to Cloud Run

From a machine with `gcloud` signed in:

```sh
gcloud run deploy recipe --source . --region europe-west1 --allow-unauthenticated
```

Or connect the GitHub repo in the Cloud Run console ("Continuously deploy from a
repository", build type Dockerfile) so every push to `main` deploys.

### One-time setup for the admin pages

Run these in [Cloud Shell](https://shell.cloud.google.com), choosing your own
password. The service is `recipe` in `europe-west1`.

```sh
PROJECT=$(gcloud config get-value project)
BUCKET=recipe-data-$PROJECT
gcloud storage buckets create gs://$BUCKET --location=europe-west1 --uniform-bucket-level-access

# Let the service's runtime account read and write the bucket.
SA=$(gcloud run services describe recipe --region europe-west1 \
  --format='value(spec.template.spec.serviceAccountName)')
SA=${SA:-$(gcloud projects describe $PROJECT --format='value(projectNumber)')-compute@developer.gserviceaccount.com}
gcloud storage buckets add-iam-policy-binding gs://$BUCKET \
  --member=serviceAccount:$SA --role=roles/storage.objectAdmin

# Mount the bucket, point the app at it, set the password, keep one instance.
gcloud run services update recipe --region europe-west1 \
  --execution-environment=gen2 \
  --add-volume=name=data,type=cloud-storage,bucket=$BUCKET,mount-options="uid=65532;gid=65532" \
  --add-volume-mount=volume=data,mount-path=/data \
  --update-env-vars=BACKUP_PATH=/data/recipes.db,ADMIN_PASSWORD='choose-a-long-password' \
  --max-instances=1
```

Later deploys from `main` keep these settings.
