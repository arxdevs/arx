# Outgoing webhooks

arx can POST a signed JSON event to a URL you register whenever a lifecycle event
happens (deploy, restart, rollback, backup). Use this to drive CI, dashboards, or
chat notifications. Endpoints are **workspace-scoped** and managed by workspace
**admins** (a non-admin member cannot create or test them).

Context: `-w` / `ARX_WORKSPACE`.

## Manage endpoints

```bash
arx webhook create --url https://example.com/hook          # subscribe to all events
arx webhook create --url https://example.com/hook \
    --events deployment.failed,backup.failed               # only these events
arx webhook create --url https://example.com/hook --project blog   # one project only
arx webhook list
arx webhook show <id>
arx webhook update <id> --url https://new --events deployment.succeeded --active true
arx webhook delete <id>
arx webhook enable <id>          # re-arm an endpoint that was auto-disabled
```

`create` prints a **signing secret once** — capture it; it is never shown again.
Pass `--secret` to supply your own. The endpoint URL must be public-ish: arx refuses
to deliver to loopback, link-local, or cloud-metadata addresses, though other hosts
on your own LAN are allowed.

## Test and inspect

```bash
arx webhook test <id>                         # queue a `test` event
arx webhook deliveries <id>                    # recent delivery attempts + status
arx webhook redeliver <id> <delivery-id>       # re-queue a past delivery
```

Delivery is asynchronous: `test` queues an event that the daemon's worker sends
shortly after. Check `deliveries` to see whether it succeeded.

## Event types

`deployment.started`, `deployment.succeeded`, `deployment.failed`,
`deployment.rolling_back`, `deployment.rolled_back`, `deployment.rollback_failed`,
`service.restarting`, `service.restarted`, `service.restart_failed`,
`backup.succeeded`, `backup.failed`, and `test`.

## Verifying the signature (receiver side)

Each POST carries `X-Arx-Event`, `X-Arx-Delivery` (stable across retries — dedupe on
it), `X-Arx-Timestamp` (Unix seconds), and `X-Arx-Signature-256`. The signature is
`sha256=` + HMAC-SHA256 over the bytes `"<timestamp>.<body>"` using the signing
secret. The receiver should recompute it and reject deliveries whose timestamp is
outside a tolerance window (e.g. 5 minutes) to prevent replay. Failed deliveries are
retried with exponential backoff; a persistently failing endpoint is auto-disabled
(re-enable with `arx webhook enable`).
