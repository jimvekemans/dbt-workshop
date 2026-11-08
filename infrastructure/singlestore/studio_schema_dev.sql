-- Workshop Studio: DEV-only landing objects (applied to analytics_dev only, after studio_schema.sql).
--
-- The QA team runs automated smoke tests against the DEV webshop. Their test orders land in the DEV feeds like any
-- other order - dirty data that PRD never has: customer ids of QA test accounts (which are not in the customer
-- feed) and purchase timestamps on the wall clock instead of the business clock.
-- The QA account registry exists in DEV only; dbt reads it through a source that is enabled in DEV only
-- (models/edw_dv_bdv/dev_only/), so PRD never parses or queries it.

USE analytics_dev;

CREATE ROWSTORE TABLE IF NOT EXISTS _qa_test_accounts (
    account_id VARCHAR(64) PRIMARY KEY,
    purpose    VARCHAR(255),
    created_at DATETIME
);
INSERT IGNORE INTO _qa_test_accounts VALUES
    ('qa-account-001', 'checkout smoke test', '2026-01-05 08:00:00'),
    ('qa-account-002', 'payment provider sandbox', '2026-01-05 08:00:00'),
    ('qa-account-003', 'fraud rule regression', '2026-02-12 08:00:00'),
    ('qa-account-004', 'load test', '2026-03-01 08:00:00'),
    ('qa-account-005', 'mobile app UI test', '2026-04-20 08:00:00');

DELIMITER //
-- One QA smoke-test run: p_orders test orders on the QA accounts, stamped with the wall clock (NOW(), years after the
-- 2017 business clock). They are not in raw_olist_customers: QA accounts live in the QA user store.
CREATE OR REPLACE PROCEDURE sp_qa_smoke_test(p_orders INT) AS
DECLARE
    v_now DATETIME = NOW();
BEGIN
    INSERT INTO raw_olist_orders
    SELECT CONCAT('qa-', DATE_FORMAT(v_now, '%Y%m%d%H%i%s'), '-', LPAD(s.n, 4, '0')),
           a.account_id, 'processing', v_now, v_now, NULL, NULL, DATE_ADD(v_now, INTERVAL 7 DAY), v_now
    FROM _seq s
    INNER JOIN (SELECT account_id, ROW_NUMBER() OVER (ORDER BY account_id) - 1 AS i FROM _qa_test_accounts) a
        ON a.i = s.n % 5
    WHERE s.n < p_orders;
END //

-- Remove the QA test orders again (a landing reset truncates them as well).
CREATE OR REPLACE PROCEDURE sp_qa_cleanup() AS
BEGIN
    DELETE FROM raw_olist_orders WHERE customer_id IN (SELECT account_id FROM _qa_test_accounts);
END //
DELIMITER ;
