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
