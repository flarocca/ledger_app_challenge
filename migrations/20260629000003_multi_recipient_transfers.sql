-- Multi-recipient transfers.
--
-- sp_transfer now takes parallel arrays of recipient account ids and amounts.
-- One operation row, one debit action (sender, -SUM(amounts)), N credit actions
-- (one per recipient, +amount[i]). Invariants preserved: SUM(actions.amount) = 0
-- per operation, and the sender's post-write balance is >= 0.
--
-- Lock order: sender + every recipient account, sorted ascending — same
-- deadlock-avoidance scheme as sp_genesis_issue.

DROP FUNCTION IF EXISTS sp_transfer(BIGINT, BIGINT, BIGINT, CHAR(3), UUID, UUID, BIGINT);

CREATE OR REPLACE FUNCTION sp_transfer(
    p_sender_account_id BIGINT,
    p_recipient_account_ids BIGINT[],
    p_amounts BIGINT[],
    p_currency CHAR(3),
    p_request_id UUID,
    p_session_id UUID,
    p_originator_user_id BIGINT
) RETURNS TABLE(out_operation_id UUID, out_sender_balance BIGINT)
LANGUAGE plpgsql AS $$
DECLARE
    v_op UUID := gen_random_uuid();
    v_count INT;
    v_distinct_count INT;
    v_total BIGINT := 0;
    v_amount BIGINT;
    v_recipient_id BIGINT;
    v_sender_balance BIGINT;
    v_sender_currency CHAR(3);
    v_recipient_balance BIGINT;
    v_recipient_currency CHAR(3);
    v_lock_ids BIGINT[];
    v_lock_id BIGINT;
    v_system_count INT;
    v_actions_sum BIGINT;
    v_i INT;
BEGIN
    v_count := COALESCE(cardinality(p_recipient_account_ids), 0);
    IF v_count = 0 THEN
        RAISE EXCEPTION 'NO_RECIPIENTS';
    END IF;
    IF v_count <> COALESCE(cardinality(p_amounts), 0) THEN
        RAISE EXCEPTION 'RECIPIENT_AMOUNT_COUNT_MISMATCH';
    END IF;

    SELECT COUNT(DISTINCT x) INTO v_distinct_count FROM unnest(p_recipient_account_ids) AS x;
    IF v_distinct_count <> v_count THEN
        RAISE EXCEPTION 'DUPLICATE_RECIPIENT';
    END IF;

    FOR v_i IN 1..v_count LOOP
        v_amount := p_amounts[v_i];
        IF v_amount IS NULL OR v_amount <= 0 THEN
            RAISE EXCEPTION 'INVALID_AMOUNT';
        END IF;
        v_total := v_total + v_amount;
        IF p_recipient_account_ids[v_i] = p_sender_account_id THEN
            RAISE EXCEPTION 'SELF_TRANSFER';
        END IF;
    END LOOP;

    SELECT COUNT(*) INTO v_system_count
        FROM accounts a JOIN users u ON u.id = a.user_id
        WHERE a.id = ANY(p_recipient_account_ids || ARRAY[p_sender_account_id])
          AND u.is_system = TRUE;
    IF v_system_count > 0 THEN
        RAISE EXCEPTION 'SYSTEM_ACCOUNT_NOT_ALLOWED';
    END IF;

    SELECT array_agg(x ORDER BY x) INTO v_lock_ids
        FROM unnest(p_recipient_account_ids || ARRAY[p_sender_account_id]) AS x;

    FOREACH v_lock_id IN ARRAY v_lock_ids LOOP
        PERFORM 1 FROM account_balances WHERE account_id = v_lock_id FOR UPDATE;
    END LOOP;

    SELECT balance, currency INTO v_sender_balance, v_sender_currency
        FROM account_balances WHERE account_id = p_sender_account_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ACCOUNT_NOT_FOUND_SENDER';
    END IF;
    IF v_sender_currency <> p_currency THEN
        RAISE EXCEPTION 'CURRENCY_MISMATCH';
    END IF;
    IF v_sender_balance < v_total THEN
        RAISE EXCEPTION 'INSUFFICIENT_FUNDS';
    END IF;

    v_sender_balance := v_sender_balance - v_total;

    INSERT INTO operations(id, kind, request_id, session_id, originator_user_id)
        VALUES (v_op, 'transfer', p_request_id, p_session_id, p_originator_user_id);

    INSERT INTO actions(operation_id, account_id, amount, resulting_balance, currency)
        VALUES (v_op, p_sender_account_id, -v_total, v_sender_balance, p_currency);

    UPDATE account_balances
        SET balance = v_sender_balance, last_operation_id = v_op, updated_at = NOW()
        WHERE account_id = p_sender_account_id;

    FOR v_i IN 1..v_count LOOP
        v_recipient_id := p_recipient_account_ids[v_i];
        v_amount := p_amounts[v_i];

        SELECT balance, currency INTO v_recipient_balance, v_recipient_currency
            FROM account_balances WHERE account_id = v_recipient_id;
        IF NOT FOUND THEN
            RAISE EXCEPTION 'ACCOUNT_NOT_FOUND_RECIPIENT';
        END IF;
        IF v_recipient_currency <> p_currency THEN
            RAISE EXCEPTION 'CURRENCY_MISMATCH';
        END IF;

        v_recipient_balance := v_recipient_balance + v_amount;

        INSERT INTO actions(operation_id, account_id, amount, resulting_balance, currency)
            VALUES (v_op, v_recipient_id, v_amount, v_recipient_balance, p_currency);

        UPDATE account_balances
            SET balance = v_recipient_balance, last_operation_id = v_op, updated_at = NOW()
            WHERE account_id = v_recipient_id;
    END LOOP;

    IF v_sender_balance < 0 THEN
        RAISE EXCEPTION 'POST_ASSERT_NEGATIVE_BALANCE';
    END IF;
    SELECT COALESCE(SUM(amount), 0) INTO v_actions_sum FROM actions WHERE operation_id = v_op;
    IF v_actions_sum <> 0 THEN
        RAISE EXCEPTION 'POST_ASSERT_NON_ZERO_SUM';
    END IF;

    RETURN QUERY SELECT v_op, v_sender_balance;
END
$$;
