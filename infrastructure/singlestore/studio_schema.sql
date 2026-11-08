-- Workshop Studio: per-channel fault injection for the Olist trickle feeder.
--
-- Template: __DB__ (analytics_dev | analytics_prod) and __ENV__ ('dev' | 'prod') are substituted by
-- bootstrap_stack.apply_studio_schema(). Idempotent: safe to re-run on an existing stack.
--
-- Public contract kept from the original init_s2.sql:
--   sp_trickle_olist_batch(days_step INT, load_domain_b_reviews BOOL)
--   sp_reset_olist_landing(initial_days INT, load_domain_b_reviews BOOL)
-- With the default channel config (all channels healthy) the landed Olist rows are identical to the original procedure.
--
-- Channels: three Olist feeds (webapp, kafka, drive) and two GENERATED feeds that Olist itself does not have:
--   carrier : the logistics partner's tracking API -> raw_carrier_tracking_events (one JSON payload per scan)
--   web     : the web-analytics team's tag manager -> raw_web_events (clickstream)
-- Both are derived deterministically from _olist_master (carrier scans from the carrier/delivery dates, page views
-- from the purchase timestamps), so they join with the Olist feeds the way real sources would - including where the
-- two teams' definitions differ. The derived archives live in _olist_master.carrier_events / _olist_master.web_events
-- and are built on the first landing reset (sp_build_generated_archives).

USE __DB__;

CREATE TABLE IF NOT EXISTS _source_channel_config (
    channel        VARCHAR(16) PRIMARY KEY,   -- 'webapp' | 'kafka' | 'drive' | 'carrier' | 'web'
    enabled        BOOL NOT NULL DEFAULT 1,
    delay_batches  INT  NOT NULL DEFAULT 0,   -- hold batches back, release later
    duplicate_pct  INT  NOT NULL DEFAULT 0,   -- re-insert X% of the batch
    partial_pct    INT  NOT NULL DEFAULT 100, -- only X% of the batch lands
    null_key_pct   INT  NOT NULL DEFAULT 0,   -- health: null business keys
    schema_variant VARCHAR(8) NOT NULL DEFAULT 'v1',
    late_pct       INT  NOT NULL DEFAULT 0,   -- carrier: X% of the non-final scans are uploaded 2 batches late
    bot_events     INT  NOT NULL DEFAULT 0,   -- web: a crawler adds N page views per batch (one visitor id)
    updated_at     DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Generated feeds: landing tables (the Olist landing tables are created by init_s2.sql).
CREATE TABLE IF NOT EXISTS raw_carrier_tracking_events (
    event_id     VARCHAR(64),
    payload      JSON,          -- v1: flat, local time / v2: camelCase, nested status code, UTC (ISO-8601)
    _ingested_at DATETIME,
    SHARD KEY (event_id),
    SORT KEY (_ingested_at)
);

CREATE TABLE IF NOT EXISTS raw_web_events (
    event_id     VARCHAR(64),
    visitor_id   VARCHAR(64),   -- first-party cookie
    customer_ref VARCHAR(64),   -- logged-in customer (v1: the webshop customer_id, v2: the account = Olist customer_unique_id)
    event_type   VARCHAR(32),   -- page_view | add_to_cart | checkout
    page         VARCHAR(255),
    referrer     VARCHAR(32),   -- traffic source of the visit: google | direct | newsletter | instagram | comparison_site
    event_ts     DATETIME,
    _ingested_at DATETIME,
    SHARD KEY (event_id),
    SORT KEY (_ingested_at)
);

-- Late carrier scans (late_pct) wait here until their release batch.
CREATE TABLE IF NOT EXISTS _held_carrier_events (
    release_at_batch INT,
    event_id         VARCHAR(64),
    payload          JSON
);

-- 0..99999: generates the crawler's page views.
CREATE ROWSTORE TABLE IF NOT EXISTS _seq (n INT PRIMARY KEY);
INSERT IGNORE INTO _seq (n)
SELECT a.d + 10 * b.d + 100 * c.d + 1000 * e.d + 10000 * f.d
FROM (SELECT 0 AS d UNION ALL SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9) a
CROSS JOIN (SELECT 0 AS d UNION ALL SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9) b
CROSS JOIN (SELECT 0 AS d UNION ALL SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9) c
CROSS JOIN (SELECT 0 AS d UNION ALL SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9) e
CROSS JOIN (SELECT 0 AS d UNION ALL SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9) f;

DELIMITER //
-- Existing stacks: add the fault columns that were introduced after their _source_channel_config was created.
CREATE OR REPLACE PROCEDURE sp_ensure_channel_columns() AS
DECLARE
    n INT = 0;
BEGIN
    SELECT COUNT(*) INTO n FROM information_schema.columns
    WHERE table_schema = '__DB__' AND table_name = '_source_channel_config' AND column_name = 'late_pct';
    IF n = 0 THEN
        EXECUTE IMMEDIATE 'ALTER TABLE _source_channel_config ADD COLUMN late_pct INT NOT NULL DEFAULT 0';
    END IF;
    SELECT COUNT(*) INTO n FROM information_schema.columns
    WHERE table_schema = '__DB__' AND table_name = '_source_channel_config' AND column_name = 'bot_events';
    IF n = 0 THEN
        EXECUTE IMMEDIATE 'ALTER TABLE _source_channel_config ADD COLUMN bot_events INT NOT NULL DEFAULT 0';
    END IF;
END //
DELIMITER ;
CALL sp_ensure_channel_columns();
INSERT IGNORE INTO _source_channel_config (channel) VALUES ('webapp'), ('kafka'), ('drive'), ('carrier'), ('web');

CREATE TABLE IF NOT EXISTS _delivery_log (
    batch_seq    INT,
    channel      VARCHAR(16),
    window_start DATETIME,
    window_end   DATETIME,
    rows_landed  BIGINT,
    rows_held    BIGINT,
    faults       VARCHAR(255),
    delivered_at DATETIME
);

CREATE TABLE IF NOT EXISTS _held_batches (
    channel          VARCHAR(16),
    window_start     DATETIME,
    window_end       DATETIME,
    release_at_batch INT
);

DELIMITER //

-- The generated archives: every carrier scan and every page view the two generated feeds will ever deliver, derived
-- once from _olist_master. Landing then only filters on the window and applies the channel faults, like the Olist feeds.
CREATE OR REPLACE PROCEDURE sp_build_generated_archives() AS
DECLARE
    n BIGINT = 0;
BEGIN
    CREATE TABLE IF NOT EXISTS _olist_master.carrier_events (
        event_id    VARCHAR(64),
        order_id    VARCHAR(64),
        shipment_id VARCHAR(32),
        event_type  VARCHAR(16),   -- picked_up | in_transit | delivered
        status_code VARCHAR(4),    -- API v2 code: PU | IT | DL
        event_ts    DATETIME,      -- local business time (UTC-3), like the Olist timestamps
        SHARD KEY (order_id),
        KEY (event_ts)
    );
    CREATE TABLE IF NOT EXISTS _olist_master.web_events (
        event_id           VARCHAR(64),
        order_id           VARCHAR(64),
        visitor_id         VARCHAR(64),
        customer_id        VARCHAR(64),   -- webshop customer id of the order (order-scoped in Olist)
        customer_unique_id VARCHAR(64),   -- the person / web account
        event_type         VARCHAR(32),
        page               VARCHAR(255),
        referrer           VARCHAR(32),
        event_ts           DATETIME,
        SHARD KEY (order_id),
        KEY (event_ts)
    );

    SELECT COUNT(*) INTO n FROM _olist_master.carrier_events;
    IF n = 0 THEN
        -- Pick-up at the carrier date, one in-transit scan half-way, delivery at the customer date. A carrier never
        -- scans a delivery before the pick-up, so the few Olist orders with a customer date before (or within two hours
        -- of) the carrier date are delivered two hours after the pick-up.
        INSERT INTO _olist_master.carrier_events
        SELECT MD5(CONCAT(order_id, '|picked_up')), order_id, CONCAT('SH', UPPER(LEFT(MD5(order_id), 10))),
               'picked_up', 'PU', order_delivered_carrier_date
        FROM _olist_master.orders WHERE order_delivered_carrier_date IS NOT NULL;
        INSERT INTO _olist_master.carrier_events
        SELECT MD5(CONCAT(order_id, '|in_transit')), order_id, CONCAT('SH', UPPER(LEFT(MD5(order_id), 10))),
               'in_transit', 'IT',
               TIMESTAMPADD(SECOND,
                   GREATEST(3600, TIMESTAMPDIFF(SECOND, order_delivered_carrier_date,
                       COALESCE(GREATEST(order_delivered_customer_date, DATE_ADD(order_delivered_carrier_date, INTERVAL 2 HOUR)),
                                DATE_ADD(order_delivered_carrier_date, INTERVAL 4 DAY))) DIV 2),
                   order_delivered_carrier_date)
        FROM _olist_master.orders WHERE order_delivered_carrier_date IS NOT NULL;
        INSERT INTO _olist_master.carrier_events
        SELECT MD5(CONCAT(order_id, '|delivered')), order_id, CONCAT('SH', UPPER(LEFT(MD5(order_id), 10))),
               'delivered', 'DL', GREATEST(order_delivered_customer_date, DATE_ADD(order_delivered_carrier_date, INTERVAL 2 HOUR))
        FROM _olist_master.orders
        WHERE order_delivered_carrier_date IS NOT NULL AND order_delivered_customer_date IS NOT NULL;
    END IF;

    SELECT COUNT(*) INTO n FROM _olist_master.web_events;
    IF n = 0 THEN
        -- The buyer's visit: 2-5 page views, add_to_cart and checkout, 4 minutes apart, ending at the purchase.
        INSERT INTO _olist_master.web_events
        SELECT MD5(CONCAT(o.order_id, '|web|', s.n)), o.order_id,
               CONCAT('v', LEFT(MD5(c.customer_unique_id), 12)), o.customer_id, c.customer_unique_id,
               CASE WHEN s.n <= k.k THEN 'page_view' WHEN s.n = k.k + 1 THEN 'add_to_cart' ELSE 'checkout' END,
               CASE WHEN s.n = k.k + 2 THEN '/checkout'
                    WHEN s.n = k.k + 1 THEN '/cart'
                    WHEN s.n % 3 = 1 THEN '/home'
                    WHEN s.n % 3 = 2 THEN CONCAT('/category/', CRC32(o.order_id) % 20)
                    ELSE CONCAT('/product/', LEFT(MD5(CONCAT(o.order_id, s.n)), 8)) END,
               ELT(1 + CRC32(o.order_id) % 5, 'google', 'direct', 'newsletter', 'instagram', 'comparison_site'),
               DATE_SUB(o.order_purchase_timestamp, INTERVAL (k.k + 2 - s.n) * 4 MINUTE)
        FROM _olist_master.orders o
        INNER JOIN _olist_master.customers c ON c.customer_id = o.customer_id
        INNER JOIN (SELECT order_id, 2 + CRC32(order_id) % 4 AS k FROM _olist_master.orders) k ON k.order_id = o.order_id
        INNER JOIN __DB__._seq s ON s.n BETWEEN 1 AND k.k + 2;
        -- Anonymous browsing (no login, no purchase) two hours before each purchase: 1-3 page views.
        INSERT INTO _olist_master.web_events
        SELECT MD5(CONCAT(o.order_id, '|anon|', s.n)), o.order_id,
               CONCAT('a', LEFT(MD5(CONCAT(o.order_id, '#anon')), 12)), NULL, NULL, 'page_view',
               IF(s.n = 1, '/home', CONCAT('/category/', (CRC32(o.order_id) + s.n) % 20)),
               ELT(1 + CRC32(CONCAT(o.order_id, 'a')) % 5, 'google', 'direct', 'newsletter', 'instagram', 'comparison_site'),
               DATE_ADD(DATE_SUB(o.order_purchase_timestamp, INTERVAL 2 HOUR), INTERVAL s.n * 3 MINUTE)
        FROM _olist_master.orders o
        INNER JOIN __DB__._seq s ON s.n BETWEEN 1 AND 1 + CRC32(CONCAT(o.order_id, 'a')) % 3;
    END IF;
END //

-- carrier: scans whose event time falls in the window. v1 = flat payload in local time; v2 = camelCase keys, nested
-- status code and UTC ISO-8601 timestamps (the carrier's API v2). late_pct: that share of the non-final scans
-- (picked_up, in_transit) is uploaded two batches later - drivers' handhelds without signal.
CREATE OR REPLACE PROCEDURE sp_land_carrier_window(
    p_start DATETIME, p_end DATETIME, p_partial INT, p_dup INT, p_nullkey INT, p_variant VARCHAR(8),
    p_late INT, p_seq INT, p_now DATETIME
) AS
BEGIN
    INSERT INTO raw_carrier_tracking_events
    SELECT e.event_id,
           IF(p_variant = 'v2',
              JSON_BUILD_OBJECT('shipmentId', IF(p_nullkey > 0 AND CRC32(e.event_id) % 100 < p_nullkey, NULL, e.shipment_id),
                                'orderReference', e.order_id,
                                'status', JSON_BUILD_OBJECT('code', e.status_code),
                                'eventTime', DATE_FORMAT(DATE_ADD(e.event_ts, INTERVAL 3 HOUR), '%Y-%m-%dT%H:%i:%sZ'),
                                'apiVersion', 2),
              JSON_BUILD_OBJECT('shipment_id', IF(p_nullkey > 0 AND CRC32(e.event_id) % 100 < p_nullkey, NULL, e.shipment_id),
                                'order_ref', e.order_id,
                                'event', e.event_type,
                                'event_time', DATE_FORMAT(e.event_ts, '%Y-%m-%d %H:%i:%s'))),
           p_now
    FROM _olist_master.carrier_events e
    WHERE e.event_ts > p_start AND e.event_ts <= p_end
      AND CRC32(e.order_id) % 100 < p_partial
      AND NOT (p_late > 0 AND e.event_type <> 'delivered' AND CRC32(e.order_id) % 100 < p_late);

    IF p_late > 0 THEN
        INSERT INTO _held_carrier_events
        SELECT p_seq + 2, e.event_id,
               IF(p_variant = 'v2',
                  JSON_BUILD_OBJECT('shipmentId', e.shipment_id, 'orderReference', e.order_id,
                                    'status', JSON_BUILD_OBJECT('code', e.status_code),
                                    'eventTime', DATE_FORMAT(DATE_ADD(e.event_ts, INTERVAL 3 HOUR), '%Y-%m-%dT%H:%i:%sZ'),
                                    'apiVersion', 2),
                  JSON_BUILD_OBJECT('shipment_id', e.shipment_id, 'order_ref', e.order_id, 'event', e.event_type,
                                    'event_time', DATE_FORMAT(e.event_ts, '%Y-%m-%d %H:%i:%s')))
        FROM _olist_master.carrier_events e
        WHERE e.event_ts > p_start AND e.event_ts <= p_end
          AND CRC32(e.order_id) % 100 < p_partial
          AND e.event_type <> 'delivered' AND CRC32(e.order_id) % 100 < p_late;
    END IF;

    -- At-least-once delivery: the same scan (same event_id) is pushed again.
    IF p_dup > 0 THEN
        INSERT INTO raw_carrier_tracking_events
        SELECT event_id, payload, p_now FROM raw_carrier_tracking_events
        WHERE _ingested_at = p_now AND CRC32(event_id) % 100 < p_dup;
    END IF;
END //

-- web: page views whose event time falls in the window. v1: customer_ref = the webshop's customer_id (order-scoped);
-- v2: customer_ref = the web account (= Olist customer_unique_id). bot_events: a crawler (one visitor id) adds N
-- page views on N different pages, spread over the window. The referrer column is looked up at run time: when the
-- web team renames it in their landing table (schema drift, scenario T2), their loader writes the new name.
CREATE OR REPLACE PROCEDURE sp_land_web_window(
    p_start DATETIME, p_end DATETIME, p_partial INT, p_dup INT, p_nullkey INT, p_variant VARCHAR(8),
    p_bots INT, p_now DATETIME
) AS
DECLARE
    ref_col VARCHAR(64) = 'referrer';
    n INT = 0;
    cols VARCHAR(255) = '';
    w_start VARCHAR(32) = DATE_FORMAT(p_start, '%Y-%m-%d %H:%i:%s');
    w_end VARCHAR(32) = DATE_FORMAT(p_end, '%Y-%m-%d %H:%i:%s');
    w_now VARCHAR(32) = DATE_FORMAT(p_now, '%Y-%m-%d %H:%i:%s');
BEGIN
    SELECT COUNT(*) INTO n FROM information_schema.columns
    WHERE table_schema = '__DB__' AND table_name = 'raw_web_events' AND column_name = 'referrer';
    IF n = 0 THEN
        SELECT column_name INTO ref_col FROM information_schema.columns
        WHERE table_schema = '__DB__' AND table_name = 'raw_web_events'
          AND column_name NOT IN ('event_id', 'visitor_id', 'customer_ref', 'event_type', 'page', 'event_ts', '_ingested_at')
        LIMIT 1;
    END IF;
    cols = CONCAT('event_id, visitor_id, customer_ref, event_type, page, `', ref_col, '`, event_ts, _ingested_at');

    EXECUTE IMMEDIATE CONCAT(
        'INSERT INTO raw_web_events (', cols, ') ',
        'SELECT w.event_id, IF(', p_nullkey, ' > 0 AND CRC32(w.event_id) % 100 < ', p_nullkey, ', NULL, w.visitor_id), ',
        'IF(''', p_variant, ''' = ''v2'', w.customer_unique_id, w.customer_id), w.event_type, w.page, w.referrer, w.event_ts, ''', w_now, ''' ',
        'FROM _olist_master.web_events w WHERE w.event_ts > ''', w_start, ''' AND w.event_ts <= ''', w_end, ''' ',
        'AND CRC32(w.order_id) % 100 < ', p_partial);

    -- The tag fires twice for X% of the events.
    IF p_dup > 0 THEN
        EXECUTE IMMEDIATE CONCAT(
            'INSERT INTO raw_web_events (', cols, ') SELECT ', cols, ' FROM raw_web_events ',
            'WHERE _ingested_at = ''', w_now, ''' AND CRC32(event_id) % 100 < ', p_dup);
    END IF;

    IF p_bots > 0 THEN
        EXECUTE IMMEDIATE CONCAT(
            'INSERT INTO raw_web_events (', cols, ') ',
            'SELECT MD5(CONCAT(''bot|'', ''', w_end, ''', ''|'', s.n)), ''bot-crawler-7'', NULL, ''page_view'', ',
            'CONCAT(''/product/'', LEFT(MD5(CONCAT(''', w_end, ''', s.n)), 10)), ''direct'', ',
            'TIMESTAMPADD(SECOND, FLOOR(TIMESTAMPDIFF(SECOND, ''', w_start, ''', ''', w_end, ''') * (s.n + 1) / ', p_bots, '), ''', w_start, '''), ',
            '''', w_now, ''' FROM _seq s WHERE s.n < LEAST(', p_bots, ', 100000)');
    END IF;
END //

-- Land one (channel, window) from the _olist_master archive, applying the configured faults.
-- Faults are deterministic: they hash the business key (CRC32) so reruns/regressions are reproducible.
CREATE OR REPLACE PROCEDURE sp_land_channel_window(
    p_channel VARCHAR(16),
    p_start DATETIME,
    p_end DATETIME,
    p_partial INT,
    p_dup INT,
    p_nullkey INT,
    p_variant VARCHAR(8),
    p_late INT,
    p_bots INT,
    p_seq INT,
    p_now DATETIME
) AS
BEGIN
    IF p_channel = 'carrier' THEN
        CALL sp_land_carrier_window(p_start, p_end, p_partial, p_dup, p_nullkey, p_variant, p_late, p_seq, p_now);
        RETURN;
    END IF;
    IF p_channel = 'web' THEN
        CALL sp_land_web_window(p_start, p_end, p_partial, p_dup, p_nullkey, p_variant, p_bots, p_now);
        RETURN;
    END IF;
    IF p_channel = 'webapp' THEN
        -- Orders purchased in (start, end]; undelivered ones are masked so they update in a later trickle.
        -- schema_variant 'v2' emits the unknown status 'fraud_hold' for a deterministic ~5% subset.
        INSERT INTO raw_olist_orders
        SELECT
            IF(p_nullkey > 0 AND CRC32(order_id) % 100 < p_nullkey, NULL, order_id),
            customer_id,
            IF(p_variant = 'v2' AND CRC32(order_id) % 100 < 5, 'fraud_hold',
               CASE
                   WHEN order_delivered_customer_date IS NOT NULL AND order_delivered_customer_date <= p_end THEN 'delivered'
                   WHEN order_delivered_carrier_date IS NOT NULL AND order_delivered_carrier_date <= p_end THEN 'shipped'
                   ELSE 'processing'
               END),
            order_purchase_timestamp,
            order_approved_at,
            IF(order_delivered_carrier_date <= p_end, order_delivered_carrier_date, NULL),
            IF(order_delivered_customer_date <= p_end, order_delivered_customer_date, NULL),
            order_estimated_delivery_date,
            p_now
        FROM _olist_master.orders
        WHERE order_purchase_timestamp > p_start
          AND order_purchase_timestamp <= p_end
          AND CRC32(order_id) % 100 < p_partial;

        -- Duplicate delivery: re-send X% of the window's orders.
        IF p_dup > 0 THEN
            INSERT INTO raw_olist_orders
            SELECT
                order_id, customer_id,
                CASE
                    WHEN order_delivered_customer_date IS NOT NULL AND order_delivered_customer_date <= p_end THEN 'delivered'
                    WHEN order_delivered_carrier_date IS NOT NULL AND order_delivered_carrier_date <= p_end THEN 'shipped'
                    ELSE 'processing'
                END,
                order_purchase_timestamp, order_approved_at,
                IF(order_delivered_carrier_date <= p_end, order_delivered_carrier_date, NULL),
                IF(order_delivered_customer_date <= p_end, order_delivered_customer_date, NULL),
                order_estimated_delivery_date, p_now
            FROM _olist_master.orders
            WHERE order_purchase_timestamp > p_start
              AND order_purchase_timestamp <= p_end
              AND CRC32(order_id) % 100 < p_partial
              AND CRC32(order_id) % 100 < p_dup;
        END IF;

        -- Simulated CDC: orders purchased before the window that were delivered inside it.
        INSERT INTO raw_olist_orders
        SELECT
            order_id, customer_id, 'delivered',
            order_purchase_timestamp, order_approved_at,
            order_delivered_carrier_date, order_delivered_customer_date,
            order_estimated_delivery_date, p_now
        FROM _olist_master.orders
        WHERE order_purchase_timestamp <= p_start
          AND order_delivered_customer_date > p_start
          AND order_delivered_customer_date <= p_end;

        -- Customers for the newly arrived orders.
        INSERT INTO raw_olist_customers
        SELECT
            c.customer_id, c.customer_unique_id, c.customer_zip_code_prefix,
            c.customer_city, c.customer_state, p_now
        FROM _olist_master.customers c
        INNER JOIN _olist_master.orders o ON c.customer_id = o.customer_id
        WHERE o.order_purchase_timestamp > p_start
          AND o.order_purchase_timestamp <= p_end
          AND CRC32(o.order_id) % 100 < p_partial;

    ELSE
        IF p_channel = 'kafka' THEN
            -- Domain A: payments for orders purchased in the window.
            INSERT INTO raw_olist_order_payments
            SELECT
                IF(p_nullkey > 0 AND CRC32(p.order_id) % 100 < p_nullkey, NULL, p.order_id),
                p.payment_sequential, p.payment_type, p.payment_installments, p.payment_value, p_now
            FROM _olist_master.order_payments p
            INNER JOIN _olist_master.orders o ON p.order_id = o.order_id
            WHERE o.order_purchase_timestamp > p_start
              AND o.order_purchase_timestamp <= p_end
              AND CRC32(p.order_id) % 100 < p_partial;

            IF p_dup > 0 THEN
                INSERT INTO raw_olist_order_payments
                SELECT
                    p.order_id, p.payment_sequential, p.payment_type, p.payment_installments, p.payment_value, p_now
                FROM _olist_master.order_payments p
                INNER JOIN _olist_master.orders o ON p.order_id = o.order_id
                WHERE o.order_purchase_timestamp > p_start
                  AND o.order_purchase_timestamp <= p_end
                  AND CRC32(p.order_id) % 100 < p_partial
                  AND CRC32(p.order_id) % 100 < p_dup;
            END IF;
        ELSE
            -- 'drive': Domain B reviews created in the window.
            INSERT INTO raw_olist_order_reviews
            SELECT
                IF(p_nullkey > 0 AND CRC32(r.review_id) % 100 < p_nullkey, NULL, r.review_id),
                r.order_id, r.review_score, r.review_comment_title, r.review_comment_message,
                r.review_creation_date, r.review_answer_timestamp, p_now
            FROM _olist_master.order_reviews r
            WHERE r.review_creation_date > p_start
              AND r.review_creation_date <= p_end
              AND CRC32(r.review_id) % 100 < p_partial;

            -- Same file delivered twice: re-send X% of the window's reviews.
            IF p_dup > 0 THEN
                INSERT INTO raw_olist_order_reviews
                SELECT
                    r.review_id, r.order_id, r.review_score, r.review_comment_title, r.review_comment_message,
                    r.review_creation_date, r.review_answer_timestamp, p_now
                FROM _olist_master.order_reviews r
                WHERE r.review_creation_date > p_start
                  AND r.review_creation_date <= p_end
                  AND CRC32(r.review_id) % 100 < p_partial
                  AND CRC32(r.review_id) % 100 < p_dup;
            END IF;
        END IF;
    END IF;
END //

-- One channel of one trickle batch: release due held windows, then deliver (or hold / skip) the current window.
CREATE OR REPLACE PROCEDURE sp_run_channel(
    p_channel VARCHAR(16),
    p_start DATETIME,
    p_end DATETIME,
    p_seq INT,
    p_now DATETIME,
    p_active BOOL
) AS
DECLARE
    c_enabled BOOL = 1;
    c_delay INT = 0;
    c_dup INT = 0;
    c_partial INT = 100;
    c_nullkey INT = 0;
    c_variant VARCHAR(8) = 'v1';
    c_late INT = 0;
    c_bots INT = 0;
    v_before BIGINT = 0;
    v_after BIGINT = 0;
    v_held BIGINT = 0;
    v_late BIGINT = 0;
    v_faults VARCHAR(255) = '';
    q QUERY(ws DATETIME, we DATETIME) = SELECT window_start, window_end FROM _held_batches
        WHERE channel = p_channel AND release_at_batch <= p_seq ORDER BY window_start;
BEGIN
    SELECT enabled INTO c_enabled FROM _source_channel_config WHERE channel = p_channel;
    SELECT delay_batches INTO c_delay FROM _source_channel_config WHERE channel = p_channel;
    SELECT duplicate_pct INTO c_dup FROM _source_channel_config WHERE channel = p_channel;
    SELECT partial_pct INTO c_partial FROM _source_channel_config WHERE channel = p_channel;
    SELECT null_key_pct INTO c_nullkey FROM _source_channel_config WHERE channel = p_channel;
    SELECT schema_variant INTO c_variant FROM _source_channel_config WHERE channel = p_channel;
    SELECT late_pct INTO c_late FROM _source_channel_config WHERE channel = p_channel;
    SELECT bot_events INTO c_bots FROM _source_channel_config WHERE channel = p_channel;

    -- SingleStore functions cannot run SQL, so the per-channel counts are inlined.
    IF p_channel = 'webapp' THEN
        SELECT COUNT(*) INTO v_before FROM raw_olist_orders;
    ELSE
        IF p_channel = 'kafka' THEN
            SELECT COUNT(*) INTO v_before FROM raw_olist_order_payments;
        ELSE
            IF p_channel = 'carrier' THEN
                SELECT COUNT(*) INTO v_before FROM raw_carrier_tracking_events;
            ELSE
                IF p_channel = 'web' THEN
                    SELECT COUNT(*) INTO v_before FROM raw_web_events;
                ELSE
                    SELECT COUNT(*) INTO v_before FROM raw_olist_order_reviews;
                END IF;
            END IF;
        END IF;
    END IF;

    -- 0. Late scans (carrier late_pct) whose release batch has been reached are uploaded now.
    IF p_channel = 'carrier' THEN
        SELECT COUNT(*) INTO v_late FROM _held_carrier_events WHERE release_at_batch <= p_seq;
        IF v_late > 0 THEN
            INSERT INTO raw_carrier_tracking_events
            SELECT event_id, payload, p_now FROM _held_carrier_events WHERE release_at_batch <= p_seq;
            DELETE FROM _held_carrier_events WHERE release_at_batch <= p_seq;
            v_faults = CONCAT(v_faults, 'late_scans_uploaded;');
        END IF;
    END IF;

    -- 1. Late arrivals: held windows whose release batch has been reached land first (ingested "now").
    FOR r IN COLLECT(q) LOOP
        CALL sp_land_channel_window(p_channel, r.ws, r.we, c_partial, c_dup, c_nullkey, c_variant, c_late, c_bots, p_seq, p_now);
        v_faults = CONCAT(v_faults, 'late_release;');
    END LOOP;
    DELETE FROM _held_batches WHERE channel = p_channel AND release_at_batch <= p_seq;

    -- 2. Current window.
    IF NOT p_active OR NOT c_enabled THEN
        v_faults = CONCAT(v_faults, IF(p_active, 'channel_disabled;', 'not_delivered;'));
    ELSE
        IF c_delay > 0 THEN
            INSERT INTO _held_batches VALUES (p_channel, p_start, p_end, p_seq + c_delay);
            IF p_channel = 'webapp' THEN
                SELECT COUNT(*) INTO v_held FROM _olist_master.orders
                WHERE order_purchase_timestamp > p_start AND order_purchase_timestamp <= p_end;
            ELSE
                IF p_channel = 'kafka' THEN
                    SELECT COUNT(*) INTO v_held FROM _olist_master.order_payments p
                    INNER JOIN _olist_master.orders o ON p.order_id = o.order_id
                    WHERE o.order_purchase_timestamp > p_start AND o.order_purchase_timestamp <= p_end;
                ELSE
                    IF p_channel = 'carrier' THEN
                        SELECT COUNT(*) INTO v_held FROM _olist_master.carrier_events
                        WHERE event_ts > p_start AND event_ts <= p_end;
                    ELSE
                        IF p_channel = 'web' THEN
                            SELECT COUNT(*) INTO v_held FROM _olist_master.web_events
                            WHERE event_ts > p_start AND event_ts <= p_end;
                        ELSE
                            SELECT COUNT(*) INTO v_held FROM _olist_master.order_reviews
                            WHERE review_creation_date > p_start AND review_creation_date <= p_end;
                        END IF;
                    END IF;
                END IF;
            END IF;
            v_faults = CONCAT(v_faults, 'delayed;');
        ELSE
            CALL sp_land_channel_window(p_channel, p_start, p_end, c_partial, c_dup, c_nullkey, c_variant, c_late, c_bots, p_seq, p_now);
            IF c_partial < 100 THEN v_faults = CONCAT(v_faults, 'partial;'); END IF;
            IF c_dup > 0 THEN v_faults = CONCAT(v_faults, 'duplicate;'); END IF;
            IF c_nullkey > 0 THEN v_faults = CONCAT(v_faults, 'null_keys;'); END IF;
            IF c_variant <> 'v1' THEN v_faults = CONCAT(v_faults, 'schema_', c_variant, ';'); END IF;
            IF c_late > 0 AND p_channel = 'carrier' THEN v_faults = CONCAT(v_faults, 'late_scans;'); END IF;
            IF c_bots > 0 AND p_channel = 'web' THEN v_faults = CONCAT(v_faults, 'bot_traffic;'); END IF;
        END IF;
    END IF;

    IF p_channel = 'webapp' THEN
        SELECT COUNT(*) INTO v_after FROM raw_olist_orders;
    ELSE
        IF p_channel = 'kafka' THEN
            SELECT COUNT(*) INTO v_after FROM raw_olist_order_payments;
        ELSE
            IF p_channel = 'carrier' THEN
                SELECT COUNT(*) INTO v_after FROM raw_carrier_tracking_events;
            ELSE
                IF p_channel = 'web' THEN
                    SELECT COUNT(*) INTO v_after FROM raw_web_events;
                ELSE
                    SELECT COUNT(*) INTO v_after FROM raw_olist_order_reviews;
                END IF;
            END IF;
        END IF;
    END IF;
    IF p_channel = 'carrier' THEN
        SELECT v_held + COUNT(*) INTO v_held FROM _held_carrier_events;
    END IF;

    INSERT INTO _delivery_log VALUES (
        p_seq, p_channel, p_start, p_end, v_after - v_before, v_held, v_faults, p_now
    );
END //

CREATE OR REPLACE PROCEDURE sp_trickle_olist_batch(
    days_step INT,
    load_domain_b_reviews BOOL
) AS
DECLARE
    v_old_wm DATETIME;
    v_new_wm DATETIME;
    v_seq INT;
    v_prev DATETIME;
    v_now DATETIME = NOW();
BEGIN
    SELECT current_watermark INTO v_old_wm FROM _trickle_state LIMIT 1;
    SELECT batch_seq INTO v_seq FROM _trickle_state LIMIT 1;
    -- Every batch gets its own, later _ingested_at - also when batches run within the same second - so arrival order
    -- (late releases, late scans) stays visible to the models.
    SELECT updated_at INTO v_prev FROM _trickle_state LIMIT 1;
    IF v_seq > 0 AND v_prev IS NOT NULL AND v_prev >= v_now THEN
        v_now = DATE_ADD(v_prev, INTERVAL 1 SECOND);
    END IF;
    v_seq = v_seq + 1;
    v_new_wm = DATE_ADD(v_old_wm, INTERVAL days_step DAY);

    CALL sp_run_channel('webapp', v_old_wm, v_new_wm, v_seq, v_now, TRUE);
    CALL sp_run_channel('kafka', v_old_wm, v_new_wm, v_seq, v_now, TRUE);
    -- Domain B: only trickles if enabled (TRUE in Dev, FALSE in Prod until released).
    CALL sp_run_channel('drive', v_old_wm, v_new_wm, v_seq, v_now, load_domain_b_reviews);
    -- Generated feeds (carrier tracking API, web analytics clickstream)
    CALL sp_run_channel('carrier', v_old_wm, v_new_wm, v_seq, v_now, TRUE);
    CALL sp_run_channel('web', v_old_wm, v_new_wm, v_seq, v_now, TRUE);

    UPDATE _trickle_state
    SET current_watermark = v_new_wm,
        batch_seq = v_seq,
        updated_at = v_now;
END //

CREATE OR REPLACE PROCEDURE sp_reset_channel_config() AS
BEGIN
    DELETE FROM _source_channel_config;
    INSERT INTO _source_channel_config (channel) VALUES ('webapp'), ('kafka'), ('drive'), ('carrier'), ('web');
END //

-- The generated landing tables back to their contract (a scenario may have renamed a column: schema drift).
CREATE OR REPLACE PROCEDURE sp_restore_generated_landing() AS
DECLARE
    n INT = 0;
BEGIN
    SELECT COUNT(*) INTO n FROM information_schema.columns
    WHERE table_schema = '__DB__' AND table_name = 'raw_web_events' AND column_name = 'referrer';
    IF n = 0 THEN
        DROP TABLE IF EXISTS raw_web_events;
        CREATE TABLE raw_web_events (
            event_id VARCHAR(64), visitor_id VARCHAR(64), customer_ref VARCHAR(64), event_type VARCHAR(32),
            page VARCHAR(255), referrer VARCHAR(32), event_ts DATETIME, _ingested_at DATETIME,
            SHARD KEY (event_id), SORT KEY (_ingested_at));
    END IF;
END //

CREATE OR REPLACE PROCEDURE sp_reset_olist_landing(
    initial_days INT,
    load_domain_b_reviews BOOL
) AS
BEGIN
    CALL sp_build_generated_archives();
    CALL sp_restore_generated_landing();
    TRUNCATE TABLE raw_olist_customers;
    TRUNCATE TABLE raw_olist_orders;
    TRUNCATE TABLE raw_olist_order_payments;
    TRUNCATE TABLE raw_olist_order_reviews;
    TRUNCATE TABLE raw_carrier_tracking_events;
    TRUNCATE TABLE raw_web_events;
    DELETE FROM _held_carrier_events;
    DELETE FROM _delivery_log;
    DELETE FROM _held_batches;
    DELETE FROM _trickle_state;
    INSERT INTO _trickle_state VALUES ('__ENV__', '2017-03-01 00:00:00', 0, NOW());
    IF initial_days > 0 THEN
        CALL sp_trickle_olist_batch(initial_days, load_domain_b_reviews);
    END IF;
END //
DELIMITER ;
