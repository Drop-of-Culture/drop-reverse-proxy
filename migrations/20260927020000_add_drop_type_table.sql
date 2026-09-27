-- drop_type was first created by hand: this makes it part of the schema.
-- Written to work both on a fresh database and on one where the table already exists.
CREATE TABLE IF NOT EXISTS "drop_type" (
    id   SMALLINT PRIMARY KEY,
    name VARCHAR(127) NOT NULL
);

-- ids are fixed codes the code relies on (drop.type_id), not generated values
ALTER TABLE "drop_type" ALTER COLUMN id DROP DEFAULT;
ALTER TABLE "drop_type" ALTER COLUMN name TYPE VARCHAR(127) USING trim(name);
CREATE UNIQUE INDEX IF NOT EXISTS drop_type_name_key ON "drop_type" (name);

INSERT INTO "drop_type" (id, name)
VALUES (0, 'audio_playlist'),
       (2, 'redirect')
ON CONFLICT (id) DO NOTHING;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'type_fkey' AND conrelid = '"drop"'::regclass
    ) THEN
        ALTER TABLE "drop" ADD CONSTRAINT type_fkey FOREIGN KEY (type_id) REFERENCES "drop_type" (id);
    END IF;
END $$;

-- the hand-made constraint was NOT VALID: check existing drops too
ALTER TABLE "drop" VALIDATE CONSTRAINT type_fkey;
