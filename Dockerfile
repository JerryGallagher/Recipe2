# syntax=docker/dockerfile:1

FROM rust:1.97-bookworm AS build
WORKDIR /app

# Cache dependencies separately from app code.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src

COPY src ./src
COPY templates ./templates
COPY migrations ./migrations
RUN touch src/main.rs && cargo build --release

FROM gcr.io/distroless/cc-debian12
WORKDIR /app
COPY --from=build /app/target/release/recipe-website /app/recipe-website
COPY static ./static

# Cloud Run sets PORT; the container filesystem is writable but ephemeral.
ENV DATABASE_PATH=/tmp/recipes.db
EXPOSE 8080
USER nonroot
CMD ["/app/recipe-website"]
