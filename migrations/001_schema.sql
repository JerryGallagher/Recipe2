CREATE TABLE IF NOT EXISTS recipes (
    id           INTEGER PRIMARY KEY,
    title        TEXT NOT NULL,
    description  TEXT NOT NULL,
    servings     INTEGER NOT NULL,
    prep_minutes INTEGER NOT NULL,
    cook_minutes INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ingredients (
    recipe_id INTEGER NOT NULL REFERENCES recipes(id),
    position  INTEGER NOT NULL,
    text      TEXT NOT NULL,
    PRIMARY KEY (recipe_id, position)
);

CREATE TABLE IF NOT EXISTS steps (
    recipe_id INTEGER NOT NULL REFERENCES recipes(id),
    position  INTEGER NOT NULL,
    text      TEXT NOT NULL,
    PRIMARY KEY (recipe_id, position)
);
