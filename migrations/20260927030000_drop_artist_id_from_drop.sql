-- drop.artist_id was added by hand on some databases: the artist of a drop is the
-- one of its artwork (drop.artwork_id -> artwork.artist_id), so it is redundant.
-- Its foreign key (artist_fkey) goes with it. No-op where the column doesn't exist.
ALTER TABLE "drop" DROP COLUMN IF EXISTS artist_id;
