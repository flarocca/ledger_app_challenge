CREATE OR REPLACE FUNCTION sp_transfer(
    p_sender_account_id BIGINT,
    p_recipient_account_id BIGINT,
    p_amount BIGINT,
    p_currency CHAR(3),
    p_request_id UUID,
    p_session_id UUID,
    p_originator_user_id BIGINT
) RETURNS TABLE(out_operation_id UUID, out_sender_balance BIGINT, out_recipient_balance BIGINT)
LANGUAGE plpgsql AS $$
DECLARE
    v_op UUID := gen_random_uuid();
    v_sender_balance BIGINT;
    v_recipient_balance BIGINT;
    v_sender_currency CHAR(3);
    v_recipient_currency CHAR(3);
    v_lock_a BIGINT;
    v_lock_b BIGINT;
    v_actions_sum BIGINT;
    v_system_count INT;
BEGIN
    IF p_amount <= 0 THEN
        RAISE EXCEPTION 'INVALID_AMOUNT';
    END IF;
    IF p_sender_account_id = p_recipient_account_id THEN
        RAISE EXCEPTION 'SELF_TRANSFER';
    END IF;

    SELECT COUNT(*) INTO v_system_count
        FROM accounts a JOIN users u ON u.id = a.user_id
        WHERE a.id IN (p_sender_account_id, p_recipient_account_id) AND u.is_system = TRUE;
    IF v_system_count > 0 THEN
        RAISE EXCEPTION 'SYSTEM_ACCOUNT_NOT_ALLOWED';
    END IF;

    v_lock_a := LEAST(p_sender_account_id, p_recipient_account_id);
    v_lock_b := GREATEST(p_sender_account_id, p_recipient_account_id);

    PERFORM 1 FROM account_balances WHERE account_id = v_lock_a FOR UPDATE;
    PERFORM 1 FROM account_balances WHERE account_id = v_lock_b FOR UPDATE;

    SELECT balance, currency INTO v_sender_balance, v_sender_currency
        FROM account_balances WHERE account_id = p_sender_account_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ACCOUNT_NOT_FOUND_SENDER';
    END IF;

    SELECT balance, currency INTO v_recipient_balance, v_recipient_currency
        FROM account_balances WHERE account_id = p_recipient_account_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ACCOUNT_NOT_FOUND_RECIPIENT';
    END IF;

    IF v_sender_currency <> v_recipient_currency OR v_sender_currency <> p_currency THEN
        RAISE EXCEPTION 'CURRENCY_MISMATCH';
    END IF;

    IF v_sender_balance < p_amount THEN
        RAISE EXCEPTION 'INSUFFICIENT_FUNDS';
    END IF;

    v_sender_balance := v_sender_balance - p_amount;
    v_recipient_balance := v_recipient_balance + p_amount;

    INSERT INTO operations(id, kind, request_id, session_id, originator_user_id)
        VALUES (v_op, 'transfer', p_request_id, p_session_id, p_originator_user_id);

    INSERT INTO actions(operation_id, account_id, amount, resulting_balance, currency)
        VALUES
            (v_op, p_sender_account_id, -p_amount, v_sender_balance, p_currency),
            (v_op, p_recipient_account_id, p_amount, v_recipient_balance, p_currency);

    UPDATE account_balances
        SET balance = v_sender_balance, last_operation_id = v_op, updated_at = NOW()
        WHERE account_id = p_sender_account_id;
    UPDATE account_balances
        SET balance = v_recipient_balance, last_operation_id = v_op, updated_at = NOW()
        WHERE account_id = p_recipient_account_id;

    IF v_sender_balance < 0 THEN
        RAISE EXCEPTION 'POST_ASSERT_NEGATIVE_BALANCE';
    END IF;
    SELECT COALESCE(SUM(amount), 0) INTO v_actions_sum FROM actions WHERE operation_id = v_op;
    IF v_actions_sum <> 0 THEN
        RAISE EXCEPTION 'POST_ASSERT_NON_ZERO_SUM';
    END IF;

    RETURN QUERY SELECT v_op, v_sender_balance, v_recipient_balance;
END
$$;

CREATE OR REPLACE FUNCTION sp_genesis_issue(
    p_treasury_account_id BIGINT,
    p_recipients BIGINT[],
    p_amount_each BIGINT,
    p_currency CHAR(3),
    p_request_id UUID
) RETURNS UUID
LANGUAGE plpgsql AS $$
DECLARE
    v_op UUID := gen_random_uuid();
    v_treasury_balance BIGINT;
    v_recipient_id BIGINT;
    v_recipient_balance BIGINT;
    v_total BIGINT := p_amount_each * cardinality(p_recipients);
    v_actions_sum BIGINT;
    v_sorted_ids BIGINT[];
BEGIN
    IF p_amount_each <= 0 OR cardinality(p_recipients) = 0 THEN
        RAISE EXCEPTION 'INVALID_GENESIS';
    END IF;

    SELECT array_agg(x ORDER BY x) INTO v_sorted_ids
        FROM unnest(array_append(p_recipients, p_treasury_account_id)) AS x;

    FOREACH v_recipient_id IN ARRAY v_sorted_ids LOOP
        PERFORM 1 FROM account_balances WHERE account_id = v_recipient_id FOR UPDATE;
    END LOOP;

    INSERT INTO operations(id, kind, request_id, session_id, originator_user_id)
        VALUES (v_op, 'genesis', p_request_id, NULL, NULL);

    SELECT balance INTO v_treasury_balance FROM account_balances WHERE account_id = p_treasury_account_id;
    v_treasury_balance := v_treasury_balance - v_total;

    INSERT INTO actions(operation_id, account_id, amount, resulting_balance, currency)
        VALUES (v_op, p_treasury_account_id, -v_total, v_treasury_balance, p_currency);

    UPDATE account_balances
        SET balance = v_treasury_balance, last_operation_id = v_op, updated_at = NOW()
        WHERE account_id = p_treasury_account_id;

    FOREACH v_recipient_id IN ARRAY p_recipients LOOP
        SELECT balance INTO v_recipient_balance FROM account_balances WHERE account_id = v_recipient_id;
        v_recipient_balance := v_recipient_balance + p_amount_each;

        INSERT INTO actions(operation_id, account_id, amount, resulting_balance, currency)
            VALUES (v_op, v_recipient_id, p_amount_each, v_recipient_balance, p_currency);

        UPDATE account_balances
            SET balance = v_recipient_balance, last_operation_id = v_op, updated_at = NOW()
            WHERE account_id = v_recipient_id;
    END LOOP;

    SELECT COALESCE(SUM(amount), 0) INTO v_actions_sum FROM actions WHERE operation_id = v_op;
    IF v_actions_sum <> 0 THEN
        RAISE EXCEPTION 'POST_ASSERT_NON_ZERO_SUM';
    END IF;

    RETURN v_op;
END
$$;
