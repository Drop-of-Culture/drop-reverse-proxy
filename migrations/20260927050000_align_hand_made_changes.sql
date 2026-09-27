-- Some databases were modified by hand and miss constraints the migrations create.
-- The back-office relies on foreign keys to refuse deleting rows still in use.
-- Every statement is a no-op where the database already matches the migrations.

DO $$
DECLARE
    fk RECORD;
BEGIN
    FOR fk IN
        SELECT * FROM (VALUES
            ('drop',     'artwork_id', 'artwork', 'drop_artwork_id_fkey'),
            ('playlist', 'drop_id',    'drop',    'playlist_drop_id_fkey'),
            ('redirect', 'drop_id',    'drop',    'redirect_drop_id_fkey')
        ) AS t(tbl, col, ref, name)
    LOOP
        IF NOT EXISTS (
            SELECT 1
            FROM pg_constraint c
            JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = ANY (c.conkey)
            WHERE c.contype = 'f'
              AND c.conrelid = format('%I', fk.tbl)::regclass
              AND a.attname = fk.col
        ) THEN
            EXECUTE format('ALTER TABLE %I ADD CONSTRAINT %I FOREIGN KEY (%I) REFERENCES %I (id)',
                           fk.tbl, fk.name, fk.col, fk.ref);
        END IF;
    END LOOP;
END $$;

ALTER TABLE "redirect" ALTER COLUMN link SET NOT NULL;
ALTER TABLE "drop" ALTER COLUMN name TYPE VARCHAR(255);
