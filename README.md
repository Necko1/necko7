# necko7

Backend service that connects Twitch Channel Points redemptions with Steam Market item purchases using viewers' Steam trade links.

## Configuration

Copy `.env.example` to `.env` and fill in the required values:

```bash
cp .env.example .env
```

| Variable | Description | Default / Example |
|---|---|---|
| `BIND_ADDR` | Listen address and port | `0.0.0.0:8080` |
| `RUST_LOG` | Tracing log level | `necko7=debug,info` |
| `DATABASE_USER` | PostgreSQL username | `necko_user` |
| `DATABASE_PASSWORD` | PostgreSQL password | `password` |
| `DATABASE_DB` | PostgreSQL database name | `necko7` |
| `DATABASE_URL` | PostgreSQL connection string | `postgres://necko_user:password@localhost:5432/necko7` |
| `APP_URL` | Public backend URL (no trailing slash) | `https://7.necko.moe` |
| `FRONTEND_URL` | Frontend URL for CORS (no trailing slash) | `https://f7.necko.moe` |
| `TWITCH_EVENTSUB_SECRET` | Secret string for EventSub webhook validation | |
| `TWITCH_CLIENT_ID` | Twitch application client ID | |
| `TWITCH_CLIENT_SECRET` | Twitch application client secret | |

### Twitch Developer Portal Setup

1. Create an application in the [Twitch Developer Console](https://dev.twitch.tv/console/apps).
2. Set OAuth Redirect URL to:
   ```
   ${APP_URL}/api/v1/auth/callback
   ```
   (e.g. `https://7.necko.moe/api/v1/auth/callback`).

## Running

### Docker Compose

```bash
docker compose up -d
```

Database migrations run automatically on startup.

### Local Development

1. Start database:
   ```bash
   docker compose up -d db
   ```

2. Run backend:
   ```bash
   cargo run
   ```

## Initial Setup

Until the bot account is initialized, all protected API routes return `404 Not Found`.

1. **Initialize bot account (required first step):**
   Open in browser:
   ```
   http(s)://<APP_URL>/api/v1/auth/init/bot
   ```
   Authorize the Twitch account that will send chat messages.

2. **Connect broadcaster channel:**
   ```
   http(s)://<APP_URL>/api/v1/auth/connect
   ```
   Authorize the streamer account to enable reward management and EventSub subscriptions.

3. **User / Moderator login:**
   ```
   http(s)://<APP_URL>/api/v1/auth/login
   ```

## API Docs

Swagger UI is available at:
```
http(s)://<APP_URL>/swagger-ui/
```
OpenAPI specification:
```
http(s)://<APP_URL>/api-docs/openapi.json
```

## CS2 Integration

The CS2 companion uses this backend's existing Axum API, session cookies, OWNER permissions, broadcaster relation, SQLx migrations and tracing. Migration `20260927120000_cs2_integration.sql` adds hashed one-time pairing codes, public-key devices and bounded replay sessions. It is applied by the normal startup migrator.

Routes (also exposed in Swagger):

| Route under `/api/v1` | Authentication | Behavior |
| --- | --- | --- |
| `GET /broadcasters/{channel_id}/cs2` | existing session + OWNER | Active device / GSI and heartbeat timestamps |
| `POST /broadcasters/{channel_id}/cs2/pairing` | existing session + OWNER | Replace code; five-minute expiry |
| `DELETE /broadcasters/{channel_id}/cs2` | existing session + OWNER | Revoke device and invalidate pending code |
| `POST /cs2/devices/pair` | temporary code | Atomically register public Ed25519 key |
| `POST /cs2/gsi` | exact raw-body Ed25519 signature | Validate, persist replay state, update last seen, normalize semantic transitions |
| `POST /cs2/devices/unpair` | signed `action: unpair` envelope | Device-authorized revocation |
| `POST /cs2/devices/heartbeat` | signed `action: heartbeat` envelope | Device reachability; revoked devices receive 401 |

The route's channel ID is checked against existing permissions, never trusted by itself. One active device/channel is enforced by a partial unique index. No desktop private/shared API secret is stored. Accepted GSI enters the backend-only normalized state/event layer after verification and replay commit. See [CS2 semantic events](docs/cs2-events.md) for the complete evidence, event catalog, identity rules and limits. The scripting platform consumes these normalized events through bounded workers. See [the scripting guide](docs/scripting/README.md) for projects, revisions, reward fulfillment and sandbox limits. Global governor budgets are per process; deploy behind the normal HTTPS reverse proxy with per-source limits. Full cross-project setup, security and Windows lifecycle instructions are in [the companion README](../necko7-cs2i/README.md).

Heartbeat: `POST /api/v1/cs2/devices/heartbeat` accepts the existing signed envelope with `action: "heartbeat"` and no `gsi`. It verifies exact bytes, timestamp and sequence just like GSI; revoked devices receive 401. Migration `20260927150000_cs2_heartbeat.sql` adds `last_heartbeat_at`. Status keeps `last_seen_at` as game activity, separate from heartbeat reachability.

For correlated raw/state/event inspection, set `CS2_LOG_GSI_PIPELINE=true` and enable DEBUG for `necko7::cs2` in `RUST_LOG`. The earlier `CS2_LOG_GSI_PAYLOADS=true` option remains available for raw-only output at the new module target. Without either flag, DEBUG still emits concise event-only records. Only accepted payloads are printed as pretty JSON. Every `auth` object is removed recursively before logging, even for signed clients that failed to strip it. The semantic state cache retains no raw payloads; diagnostics use existing tracing sinks and their retention policy. Normal logs remain compact; disable the environment flag after debugging.
