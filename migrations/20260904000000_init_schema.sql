-- Initial schema for the tables backed by src/repository/*.rs.
-- Uses IF NOT EXISTS so it's safe to run against a database whose tables
-- were already created some other way (e.g. local-dev/database dumps).

CREATE TABLE IF NOT EXISTS "artist" (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL
);

CREATE TABLE IF NOT EXISTS "artwork" (
    id SERIAL PRIMARY KEY,
    artist_id INTEGER NOT NULL REFERENCES "artist" (id),
    create_date TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    update_date TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    name CHARACTER(255) NOT NULL
);

CREATE TABLE IF NOT EXISTS "drop" (
    id SERIAL PRIMARY KEY,
    artwork_id INTEGER NOT NULL REFERENCES "artwork" (id),
    name VARCHAR(255) NOT NULL
);

CREATE TABLE IF NOT EXISTS "playlist" (
    id SERIAL PRIMARY KEY,
    drop_id INTEGER NOT NULL REFERENCES "drop" (id),
    create_date TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    update_date TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    name CHARACTER(255) NOT NULL
);

CREATE TABLE IF NOT EXISTS "tag" (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    create_date TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    drop_id INTEGER NOT NULL REFERENCES "drop" (id)
);
