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

Run these in [Cloud Shell](https://shell.cloud.google.com). The service is
`recipe` in `europe-west1`. The admin password is kept in Secret Manager, so it
never appears in the service's settings.

```sh
PROJECT=$(gcloud config get-value project)
BUCKET=recipe-data-$PROJECT
SA=$(gcloud run services describe recipe --region europe-west1 \
  --format='value(spec.template.spec.serviceAccountName)')
SA=${SA:-$(gcloud projects describe $PROJECT --format='value(projectNumber)')-compute@developer.gserviceaccount.com}

# A bucket the service can read and write.
gcloud storage buckets create gs://$BUCKET --location=europe-west1 --uniform-bucket-level-access
gcloud storage buckets add-iam-policy-binding gs://$BUCKET \
  --member=serviceAccount:$SA --role=roles/storage.objectAdmin

# The admin password, as a secret only the service can read.
# `read -s` asks for it without echoing it or saving it in shell history.
gcloud services enable secretmanager.googleapis.com
read -s -p "Admin password: " PW && printf '%s' "$PW" \
  | gcloud secrets create recipe-admin-password --data-file=- && unset PW
gcloud secrets add-iam-policy-binding recipe-admin-password \
  --member=serviceAccount:$SA --role=roles/secretmanager.secretAccessor

# Mount the bucket, pass in the password, keep one instance.
gcloud run services update recipe --region europe-west1 \
  --execution-environment=gen2 \
  --add-volume=name=data,type=cloud-storage,bucket=$BUCKET,mount-options="uid=65532;gid=65532" \
  --add-volume-mount=volume=data,mount-path=/data \
  --update-env-vars=BACKUP_PATH=/data/recipes.db \
  --update-secrets=ADMIN_PASSWORD=recipe-admin-password:latest \
  --max-instances=1
```

Later deploys from `main` keep these settings.

To change the password, add a new version of the secret and roll out a new
revision so the app reads it:

```sh
read -s -p "New admin password: " PW && printf '%s' "$PW" \
  | gcloud secrets versions add recipe-admin-password --data-file=- && unset PW
gcloud run services update recipe --region europe-west1 \
  --update-secrets=ADMIN_PASSWORD=recipe-admin-password:latest
```
