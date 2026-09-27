DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'tag_name_key'
    ) THEN
        ALTER TABLE "tag" ADD CONSTRAINT tag_name_key UNIQUE (name);
    END IF;
END $$;
