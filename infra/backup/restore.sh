#!/bin/sh
# Restores a backup made by backup.sh. Run it where the private key is,
# against an EMPTY database or one you mean to overwrite:
#
#   restore.sh <identity file> <myquiz-....dump.age> [media-....tar.age <media dir>]
#
# PGHOST, PGUSER, PGPASSWORD and PGDATABASE pick the target database.
# Practise this once a month on a scratch database (the restore drill).
set -eu
identity=${1:?age identity (private key) file}
dump=${2:?myquiz-*.dump.age}
age --decrypt --identity "$identity" "$dump" \
    | pg_restore --clean --if-exists --no-owner --exit-on-error --dbname "${PGDATABASE:-myquiz}"
echo "database restored from $dump"
if [ $# -ge 4 ]; then
    mkdir -p "$4"
    age --decrypt --identity "$identity" "$3" | tar -C "$4" -xf -
    echo "media restored to $4"
fi
