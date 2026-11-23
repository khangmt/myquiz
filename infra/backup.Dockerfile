# Nightly encrypted backups. Same PostgreSQL major version as the database,
# so pg_dump matches the server.
FROM postgres:16
RUN apt-get update && apt-get install -y --no-install-recommends age \
    && rm -rf /var/lib/apt/lists/*
COPY infra/backup/backup.sh infra/backup/restore.sh /usr/local/bin/
# docker-compose.yml runs it as the owner of the backup folder (BACKUP_UID).
ENTRYPOINT ["backup.sh"]
