# Ledger App Challenge

It is a ReactJS web application with a Rust backend api.

Authentication uses Basic auth schema, and cookies for auth sessions.

## API

Use Axum for the API.

Use PostgresSQL as the database, accessed through `sqlx` only. Sea-ORM was considered and dropped — at this scope the second ORM adds dependency weight without giving us anything we can't express directly in `sqlx`.

Use Log crate for logging

Use validation crate for DTO validation and error resonses

Use `utoipa` (+ `utoipa-swagger-ui` and `utoipa-axum`) to generate an OpenAPI 3.x spec from the handler signatures and serve a Swagger UI under `/docs`. DTOs and response envelope are `ToSchema`. The spec is the API's external contract.

### General design

The API works in layers, each layer responsible for a specific concern.

### Layers

The API layer is the interface with consuming apps. It is responsible for routing and parsing input and outputs. It never executes any business rules. It just ensures the request is valid and enforce minimal validations such as mandatory values and correct formatting. It converts the request (DTO) into a model (Business) and calls the service layer via the corresponding service to execute business rules. It finally parses the response into the proper response (DTO) or error.

The Service layer executes business logic, it receives a Model, process ir and returns a Model or a typed error the API layer can use to properly return the response. It never executes persistance logic, it always calls to the Repository layer. 

The Repostory layer is solely responsible for persistance concerns (read and write to the DB), nothing else. Repositories uses Entities.

### Interfaces / Traits

Each service must imolement a trait, handlers ALWAYS consume traits, never direct implementations.

The same applies for Repositories, each repository must imolement a Trait and services never use direct implementatiins of repositories.

This is also true for any external service or dependency

### Authentication

The API will use Basic auth (email and password) and cookies for user sessions.

There must be a middleware that checks cookie validity and populates user information for the rest of the request lifetime.

The cookie will use a 24h rolling window and 30 days fixed expiration. This is, session expires after 24hs of inactivity, but renews automatically if the user has some activity. Session expires after 30 days independently of user activity.

Logout (`POST /auth/logout`) is supported. It invalidates the current session server-side and clears the cookie on the client. This is the user voluntarily terminating their own session — distinct from administrative revocation, which remains out of scope.

<aside>
💡

Out of scope

Administrative session revocation is out of scope. The application does not revoke access to users; sessions are valid until the user logs out or the session expires.

In prod environments, certain accounts actions such as password change or 2FA changes requires the user to re authentincate.

</aside>

<aside>
💡

Out of scope

Multi device session. For the scope of the app each session is independent from one another. So users must logout each open session

</aside>

### Idenpotency

The API is the interface for its clients, and in this case the client is a web app users will use. 

Each request sent to the API will have to include a X-Request-Id header that is a randomly generated UUID by the Web App.

The WebApp must ensure that a retried request uses the same UUID so that the API can dedup them.

Request IDs are retained for 10 minutes, after that the request might have been evicted and will be considered new.

Request IDs are scoped to a single session, sessions are scoped to user.

**Required on every request, deduped only on mutations.** `X-Request-Id` must be present on every request — including reads — because the id is the correlation key for logs and tracing (see Tracing & Logs). The idempotent dedup behavior, however, only applies to mutations (POST/PUT/PATCH/DELETE). Reads are naturally idempotent and are not cached against the request id.

On a duplicate mutation (same `X-Request-Id` within a session, within the retention window), the API replays the original response **verbatim** — same status code, same headers (including `Set-Cookie`), same body. To support this the cache stores `(request_id, session_id) → (status_code, headers, response_body)` for the lifetime of the entry. Returning a generic "already done" would force the client to do a follow-up read to learn what happened; replay avoids that round-trip and gives the client the same result whether or not its prior call actually landed.

Dedup is implemented as a **tower middleware** (`middlewares/idempotency.rs`) layered between authentication and the route handlers. The handlers themselves know nothing about idempotency — they return their `Json(...)` like any other endpoint, and the middleware transparently captures the response, caches it, and short-circuits any retry. Only `2xx` responses are cached; non-success status codes pass through untouched (a 5xx is usually transient, a client retry should get a fresh attempt).

<aside>
💡

Idempotency store is in-memory only

The idempotency repository is **in-memory only** — there is no Postgres table backing it. `IdempotencyRepository` has a single implementation, `InMemoryIdempotencyRepository`, built on `DashMap` with a periodic sweep of expired entries.

The reasoning: a `request_id` lives for at most 10 minutes (per the retention spec), it's short-lived non-critical data, and the spec already permits eviction (*"after that the request might have been evicted and will be considered new"*). Putting this in Postgres just to read it back on every retry is a waste — every mutation would pay a DB round-trip for a record we're going to throw away ten minutes later anyway.

Known tradeoff: an API process restart wipes the cache. A client that retries a mutation across a redeploy could double-execute. At the single-instance scope of this app that's the same class of behavior the spec already allows (eviction); when scaling out, this would need to move to a shared store (Redis) — but that's the same conversation as horizontal scale in general, not a property of the idempotency design.

</aside>

### Cache

Sessions, users and request id must be cached to avoid exhausting the DB. For the scope of this App it will be an in-memory cache. Redis is an overkill for this.

Use the cached repository pattern when cache is needed over a DbRepository

A cached repository wraps the db repository and handles the caching logic on cache misses. It must implement the same interface.

### DTO vs Model vs Entity

In most cases, the three of them might look very similar anf feel redundant, but keeping them  as the boundary between layer helps enforce separation of concerns and also avoid exposing implementation details to the API.

DTOs are meant for consumers of the API. They live in `handlers/<resource>/requests.rs` and `handlers/<resource>/responses.rs`.

Models are meant for the business layer. They live in `models/<resource>.rs`. The service layer takes Models in and returns Models out.

Entities are meant for persistance. All of them live in **one file**, `repositories/entities.rs`, as a submodule of the repository layer — they're persistence types, owned by that layer, and every repo needs to see them. Repositories return Entities; services convert Entity → Model on read and construct Entity from Model + storage-only fields on write.

Conversions between layers use `From`/`Into` so `?` and `.into()` flow naturally:

- `From<Entity> for Model` — lives in the model file, since the model layer is "higher" and may know about the entity layer it consumes.
- `From<Model> for Dto` — lives in the response DTO file, since the API layer is "higher" and may know about the model layer it presents.
- `From<Dto> for Model` (request → command/credential) — lives in the request DTO file.

The entity layer never imports from models or handlers; it's the innermost layer.

### API Responses

API responses must be normalized so that they are easy to understand.

```json
{
    result: {},
    pagination: {},
    timestamp: ...,
    request_id: "",
    error: {
      code: "",
      message: "",
    }
}
```

### Database

Since this is a money management example, operations must be append-only representing accounting operations. The actual account balance state must be reconstructed by replaying the history of operations.

Since money management follows a ledger-kind of pattern, each operation must group the list of actions applied to each involved account. For example, a transfer from Account 1 to Account 2 for USD 1000 must be represented as an atomic operation that includes two actions: a withdraw of 1000 from Acc 1 and a deposit of 1000 to Acc 2. The sum of all actions included in an operation must be 0.

For convinience, it is ok to have the final account balance on each action.

To avoid reconstructing (by replaying) the current state there will be an account balance table, it is a snapshot in a point in time. The account balance row for each account must have a reference to the last operation applied

### Transactions

Since this is a money management system, it require strong consistency over operations that alter the ledger.

To enforce atomicity, use stored procedures for anything that requires modifying account balances.

Each store procedure must not assume validations and invariants were enforced, so it must:

1. Open a transaction
2. Lock the ledger for the accounts involved
3. Do validations
4. Write operations
5. Run post-write assertions over invariants
6. Commit or rollback

This a design desicion that contradicts one of the layer design desicions about having repository layer only responsible for persistance concerns. It is a reasonable trade off given the strong consistency requirements previously established.

<aside>
💡

Concurrency

Inside any ledger-mutating stored procedure, account rows are locked with `SELECT ... FOR UPDATE` in **ascending `account_id` order**. The deterministic lock order is what prevents the classic A→B / B→A deadlock when two concurrent transfers involve the same pair of accounts in opposite directions — without it, two transactions can each hold one row and wait forever for the other.

The transaction runs at the default `READ COMMITTED` isolation level. Row-level locks held until commit, combined with the post-write assertion that sender balance is `>= 0` and that the operation's actions sum to zero, give us the consistency we need without paying for `SERIALIZABLE`. This is what the 100-concurrent-transfer test exercises.

</aside>

### Tracing & Logs

The request id used for idempotency will also be used for tracing. Each operation must have associated a request id.

Each log must be associated to a request id. A query to the log dabatase by request id must allow to trace e2e all the activity related to that request.

The ultimate goal is to be able to precisely reconstruct account balances at any point in time, which must include also the user session under which the originator executed the transaction

The local `docker-compose` stack includes **Grafana** with **Loki** as the log backend, plus a **Promtail** sidecar that bind-mounts `api/logs/` and ships every JSON line into Loki. So logs can be queried and visualized locally by request id without any extra wiring.

<aside>
💡

How request_id reaches every log line

Mechanism: a **`tracing` span** opened in `middlewares/correlation_id.rs` — `info_span!("request", request_id = …, method = …, path = …)`. The middleware runs `next.run(req)` inside it with `.instrument(span)`, so the span is the current span for the entire handler/service/repository call tree, including across `.await` points (`tracing` propagates span context through tasks the same way it propagates through synchronous calls).

A custom `FormatEvent` impl in `logging.rs` walks the event's span ancestry on every emit, pulls `request_id` (and any other span fields) into the top level of the JSON line, and emits the result. Events outside any request — startup, shutdown — get `"request_id": null`.

Limitation: a `tokio::spawn(...)` from inside a request scope does NOT inherit the parent's span context unless you explicitly call `.in_current_span()` on the spawned future. None of the current code paths do per-request `tokio::spawn`, but it's the constraint to know if that changes.

`request_id` stays inside the JSON line, not promoted to a Loki label. Labels in Loki should be low-cardinality (otherwise the index explodes); a per-request UUID is the textbook example of what NOT to label. Query-time extraction (`| json | request_id="…"`) is the right access pattern, and unchanged from the previous `log + fern` setup.

</aside>

### Real-time feed

The global feed is delivered over **Server-Sent Events (SSE)**. The client opens an authenticated `GET /feed/stream` (auth via the session cookie, same as any other request). The server first sends the most recent N entries as a backfill burst (so the UI is never empty on connect), then keeps the connection open and pushes new ledger operations as they are committed.

SSE is chosen over WebSockets because the channel is one-way (server → client), the protocol has automatic reconnection with a `Last-Event-ID` resume hint, and proxies/CDNs treat it as a normal long-lived HTTP response. No separate gateway component is needed.

The push side is driven by the same transaction that commits a ledger operation — the API broadcasts to subscribers after the stored procedure returns successfully. The feed never reflects partial or rolled-back state because nothing reaches the broadcaster until commit.

### Testing

Every flow must fully tested e2e mocking only external dependencies or services. By external I mean services not controlled by us.

Do not mock the database, use docker-compose to set up a fresh empty database for testing, if needed, have tests running a cleanse before they run.

Each flow must be fully excersized E2E as a definition of done, including happy-path and all invariants.

Transaction atomicity is a MUST e2e test.

Session lifecycle (24h rolling renewal, 30d fixed expiration) must be covered e2e from the API side. Tests inject a clock so they don't depend on real wall-clock time. At minimum:

- A session within the rolling window extends its expiration on activity
- A session past the rolling window without activity is rejected
- A session past the 30d fixed expiration is rejected even if activity continued

## Money Management

This section is the authoritative reference for money-related design decisions in this app. Where these decisions intersect with another section (Database, Transactions, Idempotency, Real-time feed), the rule lives here and the other sections defer to it.

### Currency and representation

Amounts are stored and computed as `i64` minor units internally. Floating point is never used for monetary values anywhere in the stack — not in the database, not in the service layer, not on the wire, not in the UI.

Currency metadata lives in a `currencies` table — `(code CHAR(3) PRIMARY KEY, exponent SMALLINT, name TEXT)`. `accounts.currency`, `account_balances.currency` and `actions.currency` all reference it. Today only `USD` (exponent 2) is seeded; adding a new currency is a row insert, no schema change.

At process startup the API loads the table into an in-memory `CurrenciesService` and uses it as the single source of truth for currency metadata. This is also a hard validation gate — a request with an unknown currency code is rejected at the handler.

<aside>
💡

Decimal handling is server-side, end-to-end

The wire format for amounts is a **decimal string** (e.g. `"12.34"`), not an integer of minor units and not a JSON number. The API owns parsing and formatting, the client never multiplies by 100.

- **In:** the client sends `{ "amount": "12.34", "currency": "USD" }`. The handler resolves the currency through `CurrenciesService`, then calls `Money::from_decimal_str(input, currency)`. Parsing uses `rust_decimal` (no floats). Inputs are rejected if they're not parseable, are zero or negative, or carry more decimal places than the currency's `exponent` allows (e.g. `"12.345"` for USD → 400 `"more decimal places than USD allows (2 max)"`).
- **Out:** every monetary field on every response (`amount`, `balance`, `sender_balance_after`, …) is a decimal string formatted via `Money::to_decimal_string`, padded to the currency's exponent (so `"12.00"`, never `"12"`).
- **Why:** client-side decimal math is a footgun. `parseFloat` is locale-sensitive and inexact, `Math.round(x * 100)` silently truncates, and every client reimplements it differently. Putting the boundary in the API gives one place where rounding rules are defined, one place to audit, and a contract the OpenAPI doc can express in `string` schemas.

`Money` is the in-process bridge type (`{ minor_units: i64, currency: Currency }`) that carries amounts through service-layer business logic; the repository layer still talks to Postgres in `i64` + `currency_code: String` because that's the storage shape.

</aside>

### Account model

One user owns exactly one account. The 1:1 relationship is enforced by a unique constraint on `accounts(user_id)`. The ledger still operates on an `account_id` abstraction, so the model could be extended later without rewriting the schema.

<aside>
💡

Out of scope

Multiple accounts per user. The API and UI assume one account per user.

</aside>

### Genesis treasury

Initial user balances are **not** written directly to the balance snapshot. They are issued by a dedicated `treasury` system account through a real ledger operation at seed time — the **genesis operation**.

The motivation comes from blockchain ledger design: there is always an initial transfer at the genesis block. Every coin in circulation must trace back to an issuance; no money ever appears out of thin air. Applied here, this means the conservation invariant `SUM(actions.amount) = 0` holds across the **entire history** of the system, not just user-to-user transfers. The conservation test in the test suite therefore has real teeth — bypassing it would require corrupting the ledger directly.

The treasury account is not a regular user. It cannot be authenticated against, it does not appear in the global feed by default, and it cannot send or receive money outside of seeding.

### Transfer rules

A transfer is a single ledger operation consisting of exactly two actions: a withdraw from the sender's account and a deposit into the recipient's account, both with the same absolute amount, the pair summing to zero.

- Amount must be a positive integer in minor units (`> 0`)
- Sender and recipient must be different users (self-transfers are rejected at the service layer)
- Sender's resulting balance must be `>= 0` — enforced inside the stored procedure as a post-write assertion (see the concurrency callout under Transactions)
- Both accounts must hold the same currency

Each action row carries the resulting account balance, so the ledger is self-describing and reconstruction does not need to replay from origin on every read.

### Seed data

Four seeded users, each starting with a balance of $1000 USD, all issued by the treasury through the single genesis operation at seed time.

### Global feed contents

The feed surfaces **only successful transfers** and exposes the minimum needed to display them: sender username, recipient username, amount. No memo, no metadata, no internal operation id leaks to the UI.

<aside>
💡

Out of scope

Memo / note field on transfers. The schema can grow a nullable column later without breaking existing rows or callers.

</aside>

## Web App

For the Web App use NextJS to have SSR while still using a very flexible FE framework such as ReactJS.

Authentication will donde using Basic Auth with username & password

## Security

Https is a must

Store passwords as a salted hash

## Project structure

```
/infra
/migrations
/web
/api
  /src
    /middlewares
      /authentication.rs
      /correlation_id.rs
      /idempotency.rs
    /handlers
      /users
        /get.rs
        /post.rs
        /requests.rs
        /responses.rs
    /services
       /users.rs
    /repositories
      /users.rs
      /entities.rs
    /models
      /users.rs
```

All docker files, scripts and code needed to run the application must go into /infra

