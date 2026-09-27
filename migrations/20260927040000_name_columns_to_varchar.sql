-- CHARACTER(n) is fixed width: values come back padded with spaces up to n.
-- Convert the name columns to VARCHAR, removing the padding from existing rows.
-- On databases where a column already is VARCHAR this only trims trailing spaces.
ALTER TABLE "artist"   ALTER COLUMN name TYPE VARCHAR(255) USING rtrim(name);
ALTER TABLE "artwork"  ALTER COLUMN name TYPE VARCHAR(255) USING rtrim(name);
ALTER TABLE "playlist" ALTER COLUMN name TYPE VARCHAR(255) USING rtrim(name);
ALTER TABLE "redirect" ALTER COLUMN name TYPE VARCHAR(255) USING rtrim(name);
