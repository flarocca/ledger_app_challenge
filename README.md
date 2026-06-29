# Ledger App Challenge — money transfer with a live feed

A Venmo-like vertical slice: log in, send money by username, watch a live global feed of every transfer in the system.

## Stack

- **API**: Rust, [Axum](https://github.com/tokio-rs/axum), [sqlx](https://github.com/launchbadge/sqlx), PostgreSQL, Argon2id for password hashing, [utoipa](https://github.com/juhaku/utoipa) for OpenAPI.
- **Web**: Next.js 14 (App Router), TypeScript. SSE consumed via `fetch` so cookies + `X-Request-Id` headers come along.
- **Infra**: docker-compose with Postgres, Grafana, and Loki.

## Quick start

You need: Docker, Rust (`stable`), Node 20+, and `psql` on PATH (used by the seed script).

```bash
# 1. Bring up infra (Postgres, Grafana on http://localhost:3001, Loki)
cd infra && docker compose up -d

# 2. Apply migrations + seed (treasury + 4 users with $1000 each via the genesis operation). Idempotent.
./scripts/seed.sh

# 3. Start the API on :4000
cd ../api && cargo run --bin ledger-api

# 4. In another terminal, start the web app on :3000
cd ../web && npm install && npm run dev
```

Open <http://localhost:3000> and sign in as `alice` / `password123`. Open another browser (or incognito) as `bob` to see the live feed updating across sessions.

The seed script uses pre-computed argon2id hashes (distinct salt per user) baked into the script, so it does not depend on having Rust installed. The API still uses `sqlx::migrate!()` on boot as a safety net — migrations are idempotent (`CREATE … IF NOT EXISTS`) so both paths can apply without conflict.

- OpenAPI / Swagger UI: <http://localhost:4000/docs>
- Raw OpenAPI JSON: <http://localhost:4000/openapi.json>
- Grafana (anonymous read access): <http://localhost:3001>
- Health: <http://localhost:4000/health>

### Configuration

The API uses [`config-rs`](https://docs.rs/config) and reads env vars in three groups. Defaults match the local docker-compose setup, so nothing has to be set for the quick start. Env vars use the prefix + `__` + field-name pattern:

| Variable | Default | What it controls |
|---|---|---|
| `SERVER_CONFIG__DATABASE_URL` | `postgres://ledger:ledger@localhost:5432/ledger` | Postgres DSN |
| `SERVER_CONFIG__PORT` | `4000` | API listen port (host is fixed to `0.0.0.0`) |
| `SERVER_CONFIG__CORS_ALLOW_ORIGIN` | `http://localhost:3000` | Single origin allowed for credentialed CORS |
| `SESSION_CONFIG__COOKIE_NAME` | `ledger_session` | Session cookie name |
| `SESSION_CONFIG__COOKIE_SECURE` | `false` | `Secure` flag on the cookie — flip to `true` over HTTPS |
| `SESSION_CONFIG__ROLLING_WINDOW_SECS` | `86400` | Inactivity timeout; bumped on every authenticated request |
| `SESSION_CONFIG__ABSOLUTE_SECS` | `2592000` | Hard ceiling; session dies even with continuous activity |
| `SESSION_CONFIG__IDEMPOTENCY_TTL_SECS` | `600` | How long `(request_id, session_id)` responses stay cached for replay |
| `FEED_CONFIG__BACKFILL_SIZE` | `50` | Rows returned by `GET /feed` and pre-loaded before opening SSE |
| `FEED_CONFIG__BROADCAST_CAPACITY` | `1024` | Tokio broadcast channel size; slow subscribers drop oldest past this |
| `RUST_LOG` | `info` when unset | `env_logger` filter (e.g. `RUST_LOG=ledger_api=debug,sqlx=warn`) |

Each config group has a `load_from_env()` static and aggregates into `AppConfig`. The `seed.sh` script reads the unprefixed `DATABASE_URL` (it's a bash script, not the API), default same as above.

**Dotenv:** the API loads `api/.env` on startup via `dotenvy` before any config is read, so a local `.env` is the easiest way to override defaults. There's an `api/.env.example` you can copy:

```bash
cd api && cp .env.example .env
```

`.env` itself is gitignored.

### Tests

```bash
cd api && cargo test --tests -- --test-threads=1
```

Tests reset the public schema and re-run migrations between runs; pass `--test-threads=1` because the DB is shared. Six tests run:

- `concurrent_transfer::one_hundred_concurrent_transfers_preserve_invariants` — 100 transfers in parallel from one account, asserts no negative balance, conservation, action sum = 0, success count consistent with funds, and exact debit/credit match.
- `idempotency::duplicate_request_id_does_not_double_charge` — same `X-Request-Id` retried, only one operation lands, response body is byte-identical.
- `session_lifecycle::*` — four tests covering 24h rolling renewal, past-rolling rejection, past-30d rejection even with continuous activity, and immediate post-logout invalidation. All driven by an injectable `TestClock`.

## Project layout

```
/api                    Rust + Axum service. Bin: ledger-api.
/migrations             sqlx migrations: schema + stored procedures.
/web                    Next.js 14 app.
/infra                  docker-compose + Grafana/Loki configs.
/infra/scripts/seed.sh  bash seeder (treasury + 4 demo users + genesis op).
CLAUDE.md               My design notes and the decisions I'm willing to defend.
```

The API itself is layered the way `CLAUDE.md` describes:

```
handlers/   <- DTOs, validation, response envelope. No business rules.
services/   <- business logic. Trait-first; handlers never see the impl.
repositories/ <- persistence. Trait-first; cached repos wrap PG repos.
models/     <- domain types (User, Session, TransferCommand, …).
middlewares/  <- correlation_id, authentication.
```

## Design choices and the reasoning

### Money math

- **i64 minor units (cents) everywhere.** No floats anywhere in the stack — DB, Rust, TS, the JSON wire format. This is how ERC20 Token standard is implemented, a standard I am very used to.
- **Append-only ledger.** Two tables: `operations` (one row per atomic event) and `actions` (one row per per-account effect within an operation). Every operation's `SUM(actions.amount) = 0`. There is also a balance snapshot table (`account_balances`) so we don't have to replay history on every read. The snapshot is always written inside the same transaction that writes the actions.
- **Genesis treasury.** Initial balances are not written directly; the seed script creates a `treasury` system user and runs **one ledger operation** (`sp_genesis_issue`) that debits the treasury and credits each of the four users $1000. After seeding, `SUM(balance) = 0` across the entire DB and `SUM(actions.amount) = 0` across the entire history. This invariant therefore covers the full life of the system, not just user-to-user transfers — taking the test guarantee from "interesting" to "load-bearing."
- **Single currency (USD).** The schema carries a `currency` column for forward-compat but only accepts `USD`. Currency mismatch is explicitly rejected in the stored procedure.

### Concurrency

- **Stored procedure (`sp_transfer`) owns atomicity.** It opens nothing — the caller is in a transaction — but does all of: validate, deterministic lock, mutate, post-write assert, return. Errors `RAISE EXCEPTION` and the caller rolls back.
- **Deterministic lock order.** Both account balance rows are locked with `SELECT … FOR UPDATE` in ascending `account_id` order. Without this, two concurrent A→B and B→A transfers would deadlock; the standard fix is to pick any total order on the rows you'll touch and use it everywhere.
- **Read Committed.** Default isolation, plus row-level locks held until commit, plus a post-write `RAISE EXCEPTION` if the sender's resulting balance went negative or the action sum is non-zero. We don't pay for `SERIALIZABLE` because the locks + assertions already enforce what we need.

This is what the 100-concurrent-transfer test exercises. It runs against the real `sp_transfer`, against a real Postgres, against a single account starving on funds — and the asserts catch any drift.
Even though this locking mechanism might look fragile at scale, since locking occurs over acccount_ids specifically, it is an acceptable trade off, even at scale a user is not expected to
execute transactions that frequently.

### Idempotency

- **`X-Request-Id` required on every request** for log correlation. The middleware rejects mutations that are missing the header (400) and auto-generates one for reads so traces are always present.
- **Dedup applies only to mutations**, and runs as a **tower middleware** (`middlewares/idempotency.rs`) layered between authentication and the handlers. On a duplicate `(request_id, session_id)` the middleware short-circuits and replays the cached `(status, headers, body)` **verbatim**. On a miss it runs the inner handler, captures the response into bytes (1 MiB cap), and stores it before forwarding. Handlers are unaware — they just return their `Json(...)` like any other endpoint, no `get_or_run` boilerplate.
- **Replay preserves headers, including `Set-Cookie`.** The cache stores the full `HeaderMap`, so retries through the middleware (e.g. login with a session-creating `Set-Cookie`) come back identical to the original. Only successful 2xx responses are cached; non-2xx pass through untouched, on the theory that a 5xx is usually transient and a client retry should get a fresh chance.
- TTL is 10 minutes per the spec. Storage is **in-memory only** — `InMemoryIdempotencyRepository` (DashMap + periodic sweep), no Postgres table. A `request_id` lives for at most 10 minutes and the spec already permits eviction, so a DB round-trip per retry is pure waste. Tradeoff: a process restart wipes the cache, which at single-instance scope is the same class of behavior the spec allows. See the matching callout in `CLAUDE.md`.

### Caching

There is one **cached repository pattern** used in three places: users, sessions, idempotency. Each `Cached*Repository` wraps the `Pg*Repository` and implements the same trait (except idempotency repo), so the rest of the system doesn't know it's caching. Sessions and users have a short TTL; idempotency entries are cached at the same TTL the DB row carries.

This is in `repositories/users.rs`, `repositories/sessions.rs`, `repositories/idempotency.rs`.

### Auth

- Basic auth on `/auth/login` (username + password), Argon2id hash check, session cookie (`HttpOnly`, `SameSite=Lax`) on success.
- **24h rolling window, 30d absolute.** Each authenticated request bumps the rolling expiry up to the absolute ceiling. Past either, the session is dead.
- **Logout** (`POST /auth/logout`) revokes the session server-side. Distinct from administrative revocation, which stays out of scope.
- Session validation is its own middleware that populates `AuthContext` into request extensions so every handler downstream just `Extension<AuthContext>`s in.

### Real-time feed

- **SSE** at `GET /feed/stream`. One-way (server → client), reconnects for free, no separate gateway, no WebSocket framing to debug.
- The web app reads SSE via `fetch` (not `EventSource`) so cookies and `X-Request-Id` come along.
- **Broadcast happens after commit.** The transfer service publishes to a `tokio::sync::broadcast` channel only after `sp_transfer` committed successfully — so the feed never reflects rolled-back state.
- **Backfill on connect.** The home page does a `GET /feed` once to pre-fill the last N entries, then opens the stream for live updates. Dedup is done on the client by `(created_at, sender, recipient, amount)`.

### Observability

- Structured logs via **`tracing`** + `tracing-subscriber`, dispatched to two sinks: stdout (for `cargo run`) and `api/logs/ledger-api.log` (for Promtail). Each entry is a single JSON line — `timestamp`, `level`, `target`, `message`, `request_id`, plus any span fields in scope (e.g. `method`, `path`).
- **Request_id flows into every log line via a `tracing` span.** The `correlation_id` middleware opens an `info_span!("request", request_id = …, method = …, path = …)` and runs `next.run(req)` inside it with `.instrument(span)`. `tracing`'s context propagates across `.await` points automatically, so any `tracing::info!` / `tracing::error!` / sqlx query event emitted anywhere inside the request — handler, service, repository — inherits the span's fields. Events outside any request (startup, shutdown) emit `"request_id": null`.
- **Custom JSON formatter** (`api/src/logging.rs`) walks the span ancestry on every event and lifts span fields (`request_id`, `method`, `path`, …) to the top level of the JSON line. The wire shape is identical to the previous `log + fern` setup, so the Loki query and Grafana dashboard didn't have to change.
- **Request-start log line.** The middleware also emits one `tracing::info!("{method} {path}")` per request at `target=ledger_api::request`, so even at default `RUST_LOG=info` every request leaves a correlated breadcrumb.
- **Spans on every public handler / service / Pg-repository method.** Each layer's `#[tracing::instrument(skip_all, fields(…))]` adds its own context: the transfer handler carries `recipient_username` + `amount` + `currency`; `TransfersServiceImpl::transfer` adds `sender_user_id`; `PgTransfersRepository::execute_transfer` adds `recipient_account_id` + `amount_minor_units`. Because spans nest, a sqlx query log emitted deep inside `execute_transfer` carries the **union** of all those fields, plus `request_id` from the outermost middleware span — so a single log entry tells you who, what, how much, and which request it belonged to.
- **Loki ingest is live.** `docker-compose` adds a **Promtail** container that bind-mounts `api/logs/` and ships every JSON line to Loki at `http://loki:3100`. Promtail parses the JSON, lifts `level` and `target` into Loki labels, and uses the embedded `timestamp` as the line's ingest time. `request_id` stays inside the JSON line (not a label — labels should be low-cardinality), but is query-time extractable.
- **End-to-end audit query.** Pick any `request_id` (it's the `X-Request-Id` header echoed on the response) and run in Grafana:

  ```
  {app="ledger-api"} | json | request_id="<the uuid>"
  ```

  Every log line for that request — middleware breadcrumb, service-layer logs, sqlx queries (when at DEBUG level), errors — comes back. That's the audit trail end to end.

- Grafana at <http://localhost:3001> with anonymous read access. The provisioned "Request Trace" dashboard uses exactly this query with a `$request_id` template variable.

### OpenAPI

The spec is generated from the handler signatures and DTOs with `utoipa` and served at `/openapi.json`. Swagger UI is mounted at `/docs`. DTOs and the response envelope all derive `ToSchema`, so adding an endpoint also publishes the contract.

## Auditing & reconstruction

The ledger is the only source of truth. Any moment in history can be reconstructed by:

- `SELECT * FROM actions WHERE account_id = ? ORDER BY created_at` — per-account history with `resulting_balance` on every row, so a point-in-time balance is just a `WHERE created_at <= ?` lookup.
- `SELECT SUM(amount) FROM actions WHERE operation_id = ?` — must equal zero for every operation. If not, that's the smoking gun.
- `SELECT request_id, session_id, originator_user_id FROM operations WHERE id = ?` — every ledger event is traceable back to a request id (and therefore a log line) and the user session that authorised it.

The `account_balances` snapshot is a derived cache. If it ever disagrees with the replayed actions, the actions win — the snapshot can be rebuilt from them.

