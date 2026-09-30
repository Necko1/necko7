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

## Manual deliveries

The **Manual deliveries / Ручные выдачи** dashboard tab lets a channel OWNER or EDITOR choose an exact skin, review a Steam recipient and purchase parameters, and start delivery using that channel's Market account. It works with Twitch automation switched off. Recipients do not need an application login. No redemption, viewer inventory entry, Twitch notice or CS2 action is created.

Routes under `/api/v1/broadcasters/{channel_id}/manual-orders` are documented in Swagger:

| Method and suffix | Behavior |
| --- | --- |
| `GET /catalog` | Search the shared price catalog with `search`, `limit`, `offset` |
| `POST /preview` | Validate recipient/parameters and fetch the exact current minimum; no purchase |
| `GET /` | Paginated list with `search`, `status`, `tag`, `limit`, `offset` |
| `POST /` | Save an order and launch its first attempt |
| `GET /{id}`, `GET /{id}/audit` | Channel-scoped details, available actions and permanent history |
| `POST /{id}/retry` | Explicit new attempt after a confirmed failure |
| `PATCH /{id}` | Edit only description and tags in any status |
| `POST /{id}/close` | Close safely with an obligatory reason; does not cancel/refund a Market purchase |

Creation and retry require a UUID `request_id`. Reusing it with identical parameters returns the saved result; changing parameters with the same ID returns 409. `max_price` uses existing Market minor units (RUB ×100, USD/EUR ×1000), and `chance_to_transfer` is an integer from 0 through 100. Currency comes from the channel account; unknown currency is rejected. Price refreshes never increase an administrator's confirmed ceiling.

Orders have their own `manual_orders` identity and share durable `inventory_items` / `inventory_order_attempts` tracking. Every attempt saves its ceiling, chance, destination and `custom_id` before calling Market. An untouched first attempt resumes after restart. A saved attempt with an unknown result is checked using the same `custom_id` without buying again. Further attempts always require an administrator. Accepted Steam trades remain pending until Market stage 2. Active or uncertain deliveries block retry and closing; classified failures permit retry, while confirmed terminal unclassified failures permit closing only.

Deploy the backend with migration `20260930120000_manual_orders.sql` before deploying the frontend. Existing Twitch/Script identities and history remain valid. Manual audit is append-only and permanent; ordinary MANUAL logs follow normal retention and link to the order. API keys and trade tokens are excluded from those logs.

Validation uses a disposable PostgreSQL database and a localhost Market mock, never real purchases:

```powershell
# Set TEST_DATABASE_URL to a disposable database and the required AppState
# environment variables to dummy values, as described under Configuration.
cargo test --offline
cargo test --offline manual_orders -- --include-ignored --nocapture
cargo test --offline db::inventory::tests -- --include-ignored --test-threads=1
# From ../necko7-frontend:
npm run typecheck
npm run build
npx playwright test tests/manualOrders.spec.ts
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
