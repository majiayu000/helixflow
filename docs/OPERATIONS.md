# Helixflow operations runbook

This runbook is the production checklist for the single-process Helixflow
topology. The server owns the HTTP/WebSocket API, serves `web/dist`, and keeps
durable state under one data root.

## Preflight

1. Build from both lockfiles: `cargo build --release --locked` and
   `cd web && npm ci && npm run build`.
2. Set an explicit `HELIXFLOW_DATA_DIR` for managed deployments. The default is
   `$HOME/.helixflow`, never the process working directory.
3. Configure one default runtime provider with `HELIXFLOW_RUNTIME_PROVIDER`.
   Atlas and FAL credentials may coexist; the registry exposes every configured
   provider and each workspace persists its own enabled-provider selection.
   Mock mode requires both `HELIXFLOW_RUNTIME_PROVIDER=mock` and
   `HELIXFLOW_ENABLE_MOCK_PROVIDER=1`.
4. Set `HELIXFLOW_AUTH_TOKEN` before any non-loopback bind. Startup refuses an
   unauthenticated non-loopback address.
5. Run `./scripts/smoke-release.sh` to exercise the built web bundle, database,
   provider readiness, Bearer and browser-cookie authentication, HTTP routes,
   and graceful SIGTERM shutdown.

## Authentication topology

- Generate a high-entropy `HELIXFLOW_AUTH_TOKEN` that is unrelated to every
  provider credential. Non-loopback startup fails closed without it.
- CLI and automation send `Authorization: Bearer <token>`.
- Browser users open `/login`. A successful form exchange stores a derived
  12-hour `HttpOnly; SameSite=Strict` cookie; the deployment token is never put
  into local storage, JavaScript state, WebSocket URLs, or query strings.
- Query-string deployment tokens are rejected for both REST and WebSocket
  routes. Browser WebSockets authenticate with the same cookie and may then use
  a short-lived canvas ticket for workspace authorization.
- Non-loopback deployments mark the session cookie `Secure`; terminate TLS
  before exposing the service. This is a single-tenant deployment boundary,
  not per-user identity or role-based access control.

## Health and readiness

- `GET /api/health` is process liveness.
- `GET /api/ready` verifies the database, storage, and selected runtime
  provider. Route traffic only when it returns HTTP 200 with `ok=true`.
- A process can be live but deliberately unready while its selected provider is
  missing or unhealthy. Do not replace readiness checks with liveness checks.

## Durable data and backup

The default SQLite database is `$HELIXFLOW_DATA_DIR/helixflow.sqlite`; graph,
upload, and output files live below the same data root. `HELIXFLOW_DATABASE_URL`
may override the database URL, but the file data root remains authoritative.

For a consistent online SQLite backup, use SQLite's backup command rather than
copying only the main database file while WAL mode is active:

```sh
sqlite3 "$HELIXFLOW_DATA_DIR/helixflow.sqlite" \
  ".backup '$HELIXFLOW_DATA_DIR/backup-$(date +%Y%m%dT%H%M%S).sqlite'"
```

Back up the rest of `HELIXFLOW_DATA_DIR` in the same maintenance window. Test
restores with a separate data directory and a loopback-only bind before routing
traffic to them.

## Shutdown and restart

Send SIGTERM (or Ctrl-C locally). The server stops accepting connections and
drains active HTTP connections. The run engine persists remote handles and
terminal intent before publication; startup reconciles version files and
resumes or safely terminalizes durable work. Use the readiness endpoint to gate
traffic after every restart.

## Configuration bounds

- `HELIXFLOW_MAX_PARALLEL_STEPS`: integer 1–1024; invalid values fail startup.
- `HELIXFLOW_RUN_MAX_RETRIES`: integer 0–10; default 1.
- `HELIXFLOW_RUN_MAX_FIX_ATTEMPTS`: integer 0–10 when Agent fix is enabled.
- `HELIXFLOW_MAX_UPLOAD_BYTES`: integer 1–1073741824; default 16 MiB;
  invalid values fail startup.
- Provider polling intervals and timeouts are milliseconds/seconds respectively
  and are bounded by provider request deadlines.

Never place provider credentials in logs, database fixtures, graph parameters,
Agent context, or support bundles.

## Canvas performance gate

`cd web && npm run test:e2e` runs the real Chromium interaction suite. In CI it
also enforces the 4000-node gate: five cold loads must have p95 at or below
2.5 seconds and the 10-second pan/zoom track must have frame-interval p95 at or
below 32 ms. Dense low-zoom views use one Canvas overview bitmap while React
Flow continues to own the viewport and detailed editable nodes.
