#!/bin/sh
# Nightly encrypted backup (NFR-07, design section 10).
#
# Every night at BACKUP_HOUR (Vietnam time) this writes two files to
# /backups, both encrypted to BACKUP_AGE_RECIPIENT with age:
#   myquiz-<date>.dump.age    pg_dump of the database (custom format)
#   media-<date>.tar.age      the uploaded images
# The server only holds the public key, so a stolen backup (or a stolen
# server) cannot be read; the private key stays with the team.
#
# Kept: the last BACKUP_KEEP_DAYS nightly sets, plus the Sunday sets of the
# last BACKUP_KEEP_WEEKS weeks. The server itself has no backups, so the
# files must also be copied off it (see README, "Backups").
#
#   backup.sh          run forever, once a night
#   backup.sh now      one backup, then exit (for testing and before upgrades)
set -eu

: "${BACKUP_AGE_RECIPIENT:?set BACKUP_AGE_RECIPIENT (an age public key, age1...)}"
: "${PGHOST:=postgres}" "${PGUSER:=myquiz}" "${PGDATABASE:=myquiz}"
: "${BACKUP_HOUR:=3}" "${BACKUP_KEEP_DAYS:=14}" "${BACKUP_KEEP_WEEKS:=8}"
: "${BACKUP_DIR:=/backups}" "${MEDIA_DIR:=/media-files}"
export PGHOST PGUSER PGDATABASE TZ="${TZ:-Asia/Ho_Chi_Minh}"

log() { echo "$(date '+%Y-%m-%d %H:%M:%S') $*"; }

backup_once() {
    stamp=$(date +%Y%m%d-%H%M)
    tmp="$BACKUP_DIR/.partial-$stamp"
    mkdir -p "$tmp"
    pg_dump --format=custom --compress=6 --no-owner \
        | age --encrypt --recipient "$BACKUP_AGE_RECIPIENT" > "$tmp/myquiz-$stamp.dump.age"
    if [ -d "$MEDIA_DIR" ]; then
        tar -C "$MEDIA_DIR" -cf - . \
            | age --encrypt --recipient "$BACKUP_AGE_RECIPIENT" > "$tmp/media-$stamp.tar.age"
    fi
    # Only complete sets become visible, so a copy job never takes half a backup.
    mv "$tmp"/* "$BACKUP_DIR"/
    rmdir "$tmp"
    date +%s > "$BACKUP_DIR/last-success"
    log "backup $stamp done: $(du -ch "$BACKUP_DIR"/*-"$stamp".* | tail -1 | cut -f1)"
}

prune() {
    now=$(date +%s)
    for f in "$BACKUP_DIR"/myquiz-*.dump.age "$BACKUP_DIR"/media-*.tar.age; do
        [ -e "$f" ] || continue
        day=$(basename "$f" | sed -E 's/^[a-z]+-([0-9]{8})-.*/\1/')
        age_days=$(( (now - $(date -d "$day" +%s)) / 86400 ))
        weekday=$(date -d "$day" +%u)
        if [ "$age_days" -lt "$BACKUP_KEEP_DAYS" ]; then continue; fi
        if [ "$weekday" = 7 ] && [ "$age_days" -lt $((BACKUP_KEEP_WEEKS * 7)) ]; then continue; fi
        rm -f "$f" && log "pruned $(basename "$f")"
    done
    rm -rf "$BACKUP_DIR"/.partial-*
}

seconds_until_next_run() {
    now=$(date +%s)
    next=$(date -d "today $BACKUP_HOUR:00" +%s)
    [ "$next" -gt "$now" ] || next=$(date -d "tomorrow $BACKUP_HOUR:00" +%s)
    echo $((next - now))
}

mkdir -p "$BACKUP_DIR"
if [ "${1:-}" = now ]; then
    backup_once
    prune
    exit 0
fi
log "nightly backups at $BACKUP_HOUR:00 $TZ to $BACKUP_DIR"
while true; do
    sleep "$(seconds_until_next_run)"
    if backup_once; then prune; else log "BACKUP FAILED"; fi
done
