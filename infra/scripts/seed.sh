#!/usr/bin/env bash
set -euo pipefail

# Seeds the ledger DB: applies the SQL migrations and creates the treasury plus
# four demo users (alice, bob, carol, dave — password: "password123"), then
# issues the genesis operation that credits each user with $1000 from the
# treasury. Idempotent: re-running on an already-seeded DB exits early.
#
# Usage: DATABASE_URL=postgres://... ./infra/scripts/seed.sh

DATABASE_URL="${DATABASE_URL:-postgres://ledger:ledger@localhost:5432/ledger}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
MIGRATIONS_DIR="$SCRIPT_DIR/../../migrations"

if ! command -v psql >/dev/null 2>&1; then
    echo "error: psql is required and not found on PATH" >&2
    exit 1
fi

psql_run() {
    PGOPTIONS="--client-min-messages=warning" psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -q "$@"
}

echo "→ applying migrations"
for sql in "$MIGRATIONS_DIR"/*.sql; do
    echo "  $(basename "$sql")"
    psql_run -f "$sql"
done

if [[ "$(psql_run -t -A -c "SELECT 1 FROM users WHERE username = 'treasury' LIMIT 1" 2>/dev/null || echo "")" == "1" ]]; then
    echo "→ already seeded, nothing to do"
    exit 0
fi

# Argon2id hashes of "password123" — distinct salt per user.
TREASURY_HASH='$argon2id$v=19$m=19456,t=2,p=1$DhVVik+jA9ltN3URfN8SXw$ImT5X3TTctHoZYST4uYdan2PAH8DUPdb02Z7j3Ib0eQ'
ALICE_HASH='$argon2id$v=19$m=19456,t=2,p=1$ucrAsorqDsrJ7Wqim+2CIA$ZPe4Cm2Tc0wL8PfKtrRmnKgkx4CyAfIPZEtKT4SRiJI'
BOB_HASH='$argon2id$v=19$m=19456,t=2,p=1$pRQ+8jt7tjelTofZ9fzekQ$Km0gBtt7qIuUrAKSM6HdudTmwhRxB1g/gclE3udy2aw'
CAROL_HASH='$argon2id$v=19$m=19456,t=2,p=1$YOzod1x2CLBYKr1KgtXHJw$ktq5d7hBkZ2Q+KSG8vPAy/c2ofLwJNy9YR5ma8r9XXQ'
DAVE_HASH='$argon2id$v=19$m=19456,t=2,p=1$QJNXI1605P/hSQgtGP62/A$uypNF4RZrAUJgwC84dtn+d4eGAhLfXu/L8BzH0O/tR8'

echo "→ seeding users and running genesis operation"
psql_run <<SQL
DO \$do\$
DECLARE
    v_user_id BIGINT;
    v_treasury_account_id BIGINT;
    v_alice_account_id BIGINT;
    v_bob_account_id BIGINT;
    v_carol_account_id BIGINT;
    v_dave_account_id BIGINT;
    v_genesis_request_id UUID := gen_random_uuid();
BEGIN
    INSERT INTO currencies (code, exponent, name) VALUES ('USD', 2, 'United States Dollar')
        ON CONFLICT (code) DO NOTHING;

    INSERT INTO users (username, email, password_hash, is_system) VALUES
        ('treasury', 'treasury@system.local', '${TREASURY_HASH}', TRUE)
        RETURNING id INTO v_user_id;
    INSERT INTO accounts (user_id, currency) VALUES (v_user_id, 'USD')
        RETURNING id INTO v_treasury_account_id;
    INSERT INTO account_balances (account_id, currency, balance)
        VALUES (v_treasury_account_id, 'USD', 0);

    INSERT INTO users (username, email, password_hash, is_system) VALUES
        ('alice', 'alice@example.com', '${ALICE_HASH}', FALSE)
        RETURNING id INTO v_user_id;
    INSERT INTO accounts (user_id, currency) VALUES (v_user_id, 'USD')
        RETURNING id INTO v_alice_account_id;
    INSERT INTO account_balances (account_id, currency, balance)
        VALUES (v_alice_account_id, 'USD', 0);

    INSERT INTO users (username, email, password_hash, is_system) VALUES
        ('bob', 'bob@example.com', '${BOB_HASH}', FALSE)
        RETURNING id INTO v_user_id;
    INSERT INTO accounts (user_id, currency) VALUES (v_user_id, 'USD')
        RETURNING id INTO v_bob_account_id;
    INSERT INTO account_balances (account_id, currency, balance)
        VALUES (v_bob_account_id, 'USD', 0);

    INSERT INTO users (username, email, password_hash, is_system) VALUES
        ('carol', 'carol@example.com', '${CAROL_HASH}', FALSE)
        RETURNING id INTO v_user_id;
    INSERT INTO accounts (user_id, currency) VALUES (v_user_id, 'USD')
        RETURNING id INTO v_carol_account_id;
    INSERT INTO account_balances (account_id, currency, balance)
        VALUES (v_carol_account_id, 'USD', 0);

    INSERT INTO users (username, email, password_hash, is_system) VALUES
        ('dave', 'dave@example.com', '${DAVE_HASH}', FALSE)
        RETURNING id INTO v_user_id;
    INSERT INTO accounts (user_id, currency) VALUES (v_user_id, 'USD')
        RETURNING id INTO v_dave_account_id;
    INSERT INTO account_balances (account_id, currency, balance)
        VALUES (v_dave_account_id, 'USD', 0);

    PERFORM sp_genesis_issue(
        v_treasury_account_id,
        ARRAY[v_alice_account_id, v_bob_account_id, v_carol_account_id, v_dave_account_id],
        100000,
        'USD'::CHAR(3),
        v_genesis_request_id
    );
END
\$do\$;
SQL

echo "→ seeded: treasury + 4 users at \$1000 each (password: password123)"
