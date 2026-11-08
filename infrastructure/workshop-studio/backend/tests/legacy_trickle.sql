-- Verbatim copy of the ORIGINAL sp_trickle_olist_batch (pre-Workshop-Studio), used as regression oracle.
DELIMITER //
CREATE OR REPLACE PROCEDURE sp_trickle_olist_batch(
    days_step INT,
    load_domain_b_reviews BOOL
) AS
DECLARE
    v_old_wm DATETIME;
    v_new_wm DATETIME;
    v_now DATETIME = NOW();
BEGIN
    SELECT current_watermark INTO v_old_wm FROM _trickle_state LIMIT 1;
    v_new_wm = DATE_ADD(v_old_wm, INTERVAL days_step DAY);

    INSERT INTO raw_olist_orders
    SELECT
        order_id,
        customer_id,
        CASE
            WHEN order_delivered_customer_date IS NOT NULL AND order_delivered_customer_date <= v_new_wm THEN 'delivered'
            WHEN order_delivered_carrier_date IS NOT NULL AND order_delivered_carrier_date <= v_new_wm THEN 'shipped'
            ELSE 'processing'
        END AS order_status,
        order_purchase_timestamp,
        order_approved_at,
        IF(order_delivered_carrier_date <= v_new_wm, order_delivered_carrier_date, NULL),
        IF(order_delivered_customer_date <= v_new_wm, order_delivered_customer_date, NULL),
        order_estimated_delivery_date,
        v_now AS _ingested_at
    FROM _olist_master.orders
    WHERE order_purchase_timestamp > v_old_wm
      AND order_purchase_timestamp <= v_new_wm;

    INSERT INTO raw_olist_orders
    SELECT
        order_id,
        customer_id,
        'delivered' AS order_status,
        order_purchase_timestamp,
        order_approved_at,
        order_delivered_carrier_date,
        order_delivered_customer_date,
        order_estimated_delivery_date,
        v_now AS _ingested_at
    FROM _olist_master.orders
    WHERE order_purchase_timestamp <= v_old_wm
      AND order_delivered_customer_date > v_old_wm
      AND order_delivered_customer_date <= v_new_wm;

    INSERT INTO raw_olist_customers
    SELECT
        c.customer_id,
        c.customer_unique_id,
        c.customer_zip_code_prefix,
        c.customer_city,
        c.customer_state,
        v_now AS _ingested_at
    FROM _olist_master.customers c
    INNER JOIN _olist_master.orders o ON c.customer_id = o.customer_id
    WHERE o.order_purchase_timestamp > v_old_wm
      AND o.order_purchase_timestamp <= v_new_wm;

    INSERT INTO raw_olist_order_payments
    SELECT
        p.order_id,
        p.payment_sequential,
        p.payment_type,
        p.payment_installments,
        p.payment_value,
        v_now AS _ingested_at
    FROM _olist_master.order_payments p
    INNER JOIN _olist_master.orders o ON p.order_id = o.order_id
    WHERE o.order_purchase_timestamp > v_old_wm
      AND o.order_purchase_timestamp <= v_new_wm;

    IF load_domain_b_reviews THEN
        INSERT INTO raw_olist_order_reviews
        SELECT
            r.review_id,
            r.order_id,
            r.review_score,
            r.review_comment_title,
            r.review_comment_message,
            r.review_creation_date,
            r.review_answer_timestamp,
            v_now AS _ingested_at
        FROM _olist_master.order_reviews r
        WHERE r.review_creation_date > v_old_wm
          AND r.review_creation_date <= v_new_wm;
    END IF;

    UPDATE _trickle_state
    SET current_watermark = v_new_wm,
        batch_seq = batch_seq + 1,
        updated_at = v_now;
END //
DELIMITER ;
