# Database backups

Back up and restore `db` services (postgres / mysql / mongodb / redis), and
schedule recurring backups. Context: `-w`/`-p` or the env vars, plus the service
slug.

## On-demand

```bash
arx backup now web-db            # take a backup right now
arx backup list web-db           # list existing backups (note the storage_uri)
arx backup restore web-db <storage-uri>   # restore from a listed backup
```

`restore` overwrites the database with the backup's contents — confirm with the
user before running it on anything they care about.

## Scheduled

```bash
arx backup schedule-show web-db
arx backup schedule-set web-db \
    --cron "0 3 * * *" \      # default: daily at 03:00
    --retention 7 \           # keep the last N backups (default 7)
    --storage local           # where backups are stored (default local)
```

Scheduled backups run on the daemon. Set `--retention` so old backups are pruned
automatically instead of filling the disk.

## Notes

- Backups apply to `db` services only; there is nothing to back up for `git`/`image`
  services (their state lives in their own data stores or volumes).
- A successful or failed backup can fire an outgoing webhook
  (`backup.succeeded` / `backup.failed`) — see `references/webhooks.md`.
