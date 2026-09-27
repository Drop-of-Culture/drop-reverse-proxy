#!/usr/bin/env bash
# Dumps the postgres database running in the "db" docker container to a plain SQL file.
# Intended to be run by cron, e.g.:
#   0 2 * * * /path/to/utils/backup-db.sh >> /path/to/backups/backup.log 2>&1
set -euo pipefail

CONTAINER="${CONTAINER:-db}"
DB_NAME="${DB_NAME:-dropofculture}"
DB_USER="${DB_USER:-dropofculture}"
BACKUP_DIR="${BACKUP_DIR:-$HOME/backups/dropofculture}"
RETENTION_DAYS="${RETENTION_DAYS:-14}"

# cron runs with a minimal PATH
export PATH="/usr/local/bin:/usr/bin:/bin:$PATH"

mkdir -p "$BACKUP_DIR"
target="$BACKUP_DIR/${DB_NAME}_$(date +%Y-%m-%d_%H%M%S).sql"
tmp="$target.partial"

echo "$(date -Is) starting backup of $DB_NAME from container $CONTAINER"

# write to a temp file first so a failed dump never looks like a valid backup
if docker exec "$CONTAINER" pg_dump -U "$DB_USER" -d "$DB_NAME" --clean --if-exists --no-owner > "$tmp"; then
    mv "$tmp" "$target"
    echo "$(date -Is) backup written to $target ($(du -h "$target" | cut -f1))"
else
    rm -f "$tmp"
    echo "$(date -Is) backup FAILED" >&2
    exit 1
fi

find "$BACKUP_DIR" -name "${DB_NAME}_*.sql" -type f -mtime "+$RETENTION_DAYS" -print -delete
