-- 1. Cap SingleStore memory limits (4GB table memory, 5GB total engine memory) and set VDAB enterprise compatibility level
SET GLOBAL maximum_table_memory = 4096;
SET GLOBAL maximum_memory = 5120;
SET GLOBAL data_conversion_compatibility_level = '6.0';


-- 2. Create Dev, Prod, Master Archive, and VDAB Enterprise databases
CREATE DATABASE IF NOT EXISTS analytics_dev;
CREATE DATABASE IF NOT EXISTS analytics_prod;
CREATE DATABASE IF NOT EXISTS _olist_master;
CREATE DATABASE IF NOT EXISTS edw_dv_rdv;
CREATE DATABASE IF NOT EXISTS edw_dv_bdv;
CREATE DATABASE IF NOT EXISTS edw_ldm;
CREATE DATABASE IF NOT EXISTS edw_fmc;
CREATE DATABASE IF NOT EXISTS im_dimensional;
CREATE DATABASE IF NOT EXISTS pres_dataproduct;
CREATE DATABASE IF NOT EXISTS tenant_ai;
CREATE DATABASE IF NOT EXISTS vdp_mgmt;
CREATE DATABASE IF NOT EXISTS vdp_ci;

-- 3. Create RBAC Roles
CREATE ROLE 'role_viewer';
CREATE ROLE 'role_user';
CREATE ROLE 'role_maintainer';
CREATE ROLE 'role_admin';

-- Create RBAC Groups
CREATE GROUP 'grp_viewer';
CREATE GROUP 'grp_user';
CREATE GROUP 'grp_maintainer';
CREATE GROUP 'grp_admin';

-- Map Roles to Groups
GRANT ROLE 'role_viewer' TO 'grp_viewer';
GRANT ROLE 'role_user' TO 'grp_user';
GRANT ROLE 'role_maintainer' TO 'grp_maintainer';
GRANT ROLE 'role_admin' TO 'grp_admin';

-- 4. Grant privileges to roles
-- Viewer: Read-only in all databases
GRANT SELECT, SHOW VIEW ON analytics_dev.* TO ROLE 'role_viewer';
GRANT SELECT, SHOW VIEW ON analytics_prod.* TO ROLE 'role_viewer';
GRANT SELECT, SHOW VIEW ON edw_dv_rdv.* TO ROLE 'role_viewer';
GRANT SELECT, SHOW VIEW ON edw_dv_bdv.* TO ROLE 'role_viewer';
GRANT SELECT, SHOW VIEW ON edw_ldm.* TO ROLE 'role_viewer';
GRANT SELECT, SHOW VIEW ON edw_fmc.* TO ROLE 'role_viewer';
GRANT SELECT, SHOW VIEW ON im_dimensional.* TO ROLE 'role_viewer';
GRANT SELECT, SHOW VIEW ON pres_dataproduct.* TO ROLE 'role_viewer';
GRANT SELECT, SHOW VIEW ON tenant_ai.* TO ROLE 'role_viewer';
GRANT SELECT, SHOW VIEW ON vdp_mgmt.* TO ROLE 'role_viewer';
GRANT SELECT, SHOW VIEW ON vdp_ci.* TO ROLE 'role_viewer';

-- User: Read/Write in Dev & VDP schemas, Read-only in Prod
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON analytics_dev.* TO ROLE 'role_user';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON vdp_ci.* TO ROLE 'role_user';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON edw_dv_rdv.* TO ROLE 'role_user';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON edw_dv_bdv.* TO ROLE 'role_user';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON edw_ldm.* TO ROLE 'role_user';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON edw_fmc.* TO ROLE 'role_user';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON im_dimensional.* TO ROLE 'role_user';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON pres_dataproduct.* TO ROLE 'role_user';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON tenant_ai.* TO ROLE 'role_user';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON vdp_mgmt.* TO ROLE 'role_user';
GRANT SELECT, SHOW VIEW ON analytics_prod.* TO ROLE 'role_user';

-- Maintainer: Read/Write in both Dev, Prod, and all VDP databases
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON analytics_dev.* TO ROLE 'role_maintainer';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON analytics_prod.* TO ROLE 'role_maintainer';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON edw_dv_rdv.* TO ROLE 'role_maintainer';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON edw_dv_bdv.* TO ROLE 'role_maintainer';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON edw_ldm.* TO ROLE 'role_maintainer';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON edw_fmc.* TO ROLE 'role_maintainer';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON im_dimensional.* TO ROLE 'role_maintainer';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON pres_dataproduct.* TO ROLE 'role_maintainer';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON tenant_ai.* TO ROLE 'role_maintainer';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON vdp_mgmt.* TO ROLE 'role_maintainer';
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP, ALTER, INDEX, CREATE VIEW, SHOW VIEW, CREATE TEMPORARY TABLES, LOCK TABLES ON vdp_ci.* TO ROLE 'role_maintainer';

-- Master Archive: Read-only for Maintainer and Admin
GRANT SELECT ON _olist_master.* TO ROLE 'role_maintainer';
GRANT SELECT ON _olist_master.* TO ROLE 'role_admin';

-- Admin: Full cluster privileges
GRANT ALL PRIVILEGES ON *.* TO ROLE 'role_admin' WITH GRANT OPTION;

-- 5. Create the 7 Shared Users + PU-* Personal Users + Service Accounts (*_dbt)
CREATE USER IF NOT EXISTS 'admin1'@'%' IDENTIFIED BY 'WorkshopPwd2026!';
CREATE USER IF NOT EXISTS 'maintainer1'@'%' IDENTIFIED BY 'WorkshopPwd2026!';
CREATE USER IF NOT EXISTS 'maintainer2'@'%' IDENTIFIED BY 'WorkshopPwd2026!';
CREATE USER IF NOT EXISTS 'user1'@'%' IDENTIFIED BY 'WorkshopPwd2026!';
CREATE USER IF NOT EXISTS 'user2'@'%' IDENTIFIED BY 'WorkshopPwd2026!';
CREATE USER IF NOT EXISTS 'viewer1'@'%' IDENTIFIED BY 'WorkshopPwd2026!';
CREATE USER IF NOT EXISTS 'viewer2'@'%' IDENTIFIED BY 'WorkshopPwd2026!';

CREATE USER IF NOT EXISTS 'pu_admin1'@'%' IDENTIFIED BY 'WorkshopPwd2026!';
CREATE USER IF NOT EXISTS 'pu_maintainer1'@'%' IDENTIFIED BY 'WorkshopPwd2026!';
CREATE USER IF NOT EXISTS 'pu_maintainer2'@'%' IDENTIFIED BY 'WorkshopPwd2026!';
CREATE USER IF NOT EXISTS 'pu_user1'@'%' IDENTIFIED BY 'WorkshopPwd2026!';
CREATE USER IF NOT EXISTS 'pu_user2'@'%' IDENTIFIED BY 'WorkshopPwd2026!';
CREATE USER IF NOT EXISTS 'pu_viewer1'@'%' IDENTIFIED BY 'WorkshopPwd2026!';
CREATE USER IF NOT EXISTS 'pu_viewer2'@'%' IDENTIFIED BY 'WorkshopPwd2026!';

CREATE USER IF NOT EXISTS 'svc_ci'@'%' IDENTIFIED BY 'CiServicePwd2026!';
CREATE USER IF NOT EXISTS 'svc_ci_dbt'@'%' IDENTIFIED BY 'CiServicePwd2026!';
CREATE USER IF NOT EXISTS 'svc_ldv_dbt'@'%' IDENTIFIED BY 'CiServicePwd2026!';
CREATE USER IF NOT EXISTS 'svc_prd_dbt'@'%' IDENTIFIED BY 'CiServicePwd2026!';

-- 6. Bind Users to Groups (SingleStore RBAC: User -> Group -> Role -> Privileges)
GRANT GROUP 'grp_admin' TO 'admin1'@'%';
GRANT GROUP 'grp_maintainer' TO 'maintainer1'@'%';
GRANT GROUP 'grp_maintainer' TO 'maintainer2'@'%';
GRANT GROUP 'grp_user' TO 'user1'@'%';
GRANT GROUP 'grp_user' TO 'user2'@'%';
GRANT GROUP 'grp_viewer' TO 'viewer1'@'%';
GRANT GROUP 'grp_viewer' TO 'viewer2'@'%';

GRANT GROUP 'grp_admin' TO 'pu_admin1'@'%';
GRANT GROUP 'grp_maintainer' TO 'pu_maintainer1'@'%';
GRANT GROUP 'grp_maintainer' TO 'pu_maintainer2'@'%';
GRANT GROUP 'grp_user' TO 'pu_user1'@'%';
GRANT GROUP 'grp_user' TO 'pu_user2'@'%';
GRANT GROUP 'grp_viewer' TO 'pu_viewer1'@'%';
GRANT GROUP 'grp_viewer' TO 'pu_viewer2'@'%';

GRANT GROUP 'grp_admin' TO 'svc_ci'@'%';
GRANT GROUP 'grp_admin' TO 'svc_ci_dbt'@'%';
GRANT GROUP 'grp_admin' TO 'svc_ldv_dbt'@'%';
GRANT GROUP 'grp_admin' TO 'svc_prd_dbt'@'%';

FLUSH PRIVILEGES;

-- 6b. VDAB Management Tables (vdp_mgmt)
USE vdp_mgmt;

CREATE TABLE IF NOT EXISTS dbt_run_results (
    invocation_id VARCHAR(250),
    run_started_at DATETIME(6),
    unique_id VARCHAR(1000),
    file_path VARCHAR(1000),
    relation_name VARCHAR(250),
    database_name VARCHAR(250),
    name VARCHAR(1000),
    resource_type VARCHAR(250),
    materialized VARCHAR(250),
    status VARCHAR(250),
    failures VARCHAR(1000),
    started_at DATETIME(6),
    completed_at DATETIME(6),
    execution_time DECIMAL(38, 10),
    rows_affected BIGINT,
    pre_hook LONGTEXT,
    post_hook LONGTEXT,
    compiled_code LONGTEXT,
    SHARD KEY (invocation_id, name),
    PRIMARY KEY (invocation_id, name)
);

CREATE TABLE IF NOT EXISTS vdp_parameters (
    parameter_name VARCHAR(250) PRIMARY KEY,
    parameter_value VARCHAR(1000),
    parameter_description VARCHAR(1000),
    dtm_cre DATETIME,
    usr_cre VARCHAR(100),
    dtm_lw DATETIME,
    usr_lw VARCHAR(100)
);

CREATE TABLE IF NOT EXISTS qa_test_result (
    test_name VARCHAR(255),
    executed_at VARCHAR(64),
    failing_rows BIGINT,
    PRIMARY KEY (test_name, executed_at)
);

-- 7. Master Archive Tables (_olist_master)
USE _olist_master;

CREATE TABLE IF NOT EXISTS customers (
    customer_id VARCHAR(64),
    customer_unique_id VARCHAR(64),
    customer_zip_code_prefix VARCHAR(16),
    customer_city VARCHAR(64),
    customer_state VARCHAR(8),
    SHARD KEY (customer_id)
);

CREATE TABLE IF NOT EXISTS orders (
    order_id VARCHAR(64),
    customer_id VARCHAR(64),
    order_status VARCHAR(32),
    order_purchase_timestamp DATETIME,
    order_approved_at DATETIME,
    order_delivered_carrier_date DATETIME,
    order_delivered_customer_date DATETIME,
    order_estimated_delivery_date DATETIME,
    SHARD KEY (order_id),
    KEY (order_purchase_timestamp)
);

CREATE TABLE IF NOT EXISTS order_payments (
    order_id VARCHAR(64),
    payment_sequential INT,
    payment_type VARCHAR(32),
    payment_installments INT,
    payment_value DECIMAL(10,2),
    SHARD KEY (order_id)
);

CREATE TABLE IF NOT EXISTS order_reviews (
    review_id VARCHAR(64),
    order_id VARCHAR(64),
    review_score INT,
    review_comment_title TEXT,
    review_comment_message TEXT,
    review_creation_date DATETIME,
    review_answer_timestamp DATETIME,
    SHARD KEY (order_id),
    KEY (review_creation_date)
);

CREATE TABLE IF NOT EXISTS order_items (
    order_id VARCHAR(64),
    order_item_id INT,
    product_id VARCHAR(64),
    seller_id VARCHAR(64),
    shipping_limit_date DATETIME,
    price DECIMAL(10,2),
    freight_value DECIMAL(10,2),
    SHARD KEY (order_id)
);

-- 8. Watermark State Table, Raw Landing Tables & Stored Procedure for analytics_dev
USE analytics_dev;

CREATE TABLE IF NOT EXISTS _trickle_state (
    env_name VARCHAR(16) PRIMARY KEY,
    current_watermark DATETIME NOT NULL,
    batch_seq INT NOT NULL,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

INSERT IGNORE INTO _trickle_state VALUES ('dev', '2017-03-01 00:00:00', 0, NOW());

CREATE TABLE IF NOT EXISTS raw_olist_customers (
    customer_id VARCHAR(64),
    customer_unique_id VARCHAR(64),
    customer_zip_code_prefix VARCHAR(16),
    customer_city VARCHAR(64),
    customer_state VARCHAR(8),
    _ingested_at DATETIME,
    SHARD KEY (customer_id),
    SORT KEY (_ingested_at)
);

CREATE TABLE IF NOT EXISTS raw_olist_orders (
    order_id VARCHAR(64),
    customer_id VARCHAR(64),
    order_status VARCHAR(32),
    order_purchase_timestamp DATETIME,
    order_approved_at DATETIME,
    order_delivered_carrier_date DATETIME,
    order_delivered_customer_date DATETIME,
    order_estimated_delivery_date DATETIME,
    _ingested_at DATETIME,
    SHARD KEY (order_id),
    SORT KEY (_ingested_at)
);

CREATE TABLE IF NOT EXISTS raw_olist_order_payments (
    order_id VARCHAR(64),
    payment_sequential INT,
    payment_type VARCHAR(32),
    payment_installments INT,
    payment_value DECIMAL(10,2),
    _ingested_at DATETIME,
    SHARD KEY (order_id),
    SORT KEY (_ingested_at)
);

CREATE TABLE IF NOT EXISTS raw_olist_order_reviews (
    review_id VARCHAR(64),
    order_id VARCHAR(64),
    review_score INT,
    review_comment_title TEXT,
    review_comment_message TEXT,
    review_creation_date DATETIME,
    review_answer_timestamp DATETIME,
    _ingested_at DATETIME,
    SHARD KEY (order_id),
    SORT KEY (_ingested_at)
);

-- sp_trickle_olist_batch / sp_reset_olist_landing / channel fault-injection tables live in
-- singlestore/studio_schema.sql (applied for this DB by bootstrap_stack.apply_studio_schema()).

-- 9. Watermark State Table, Raw Landing Tables & Stored Procedure for analytics_prod
USE analytics_prod;

CREATE TABLE IF NOT EXISTS _trickle_state (
    env_name VARCHAR(16) PRIMARY KEY,
    current_watermark DATETIME NOT NULL,
    batch_seq INT NOT NULL,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

INSERT IGNORE INTO _trickle_state VALUES ('prod', '2017-03-01 00:00:00', 0, NOW());

CREATE TABLE IF NOT EXISTS raw_olist_customers (
    customer_id VARCHAR(64),
    customer_unique_id VARCHAR(64),
    customer_zip_code_prefix VARCHAR(16),
    customer_city VARCHAR(64),
    customer_state VARCHAR(8),
    _ingested_at DATETIME,
    SHARD KEY (customer_id),
    SORT KEY (_ingested_at)
);

CREATE TABLE IF NOT EXISTS raw_olist_orders (
    order_id VARCHAR(64),
    customer_id VARCHAR(64),
    order_status VARCHAR(32),
    order_purchase_timestamp DATETIME,
    order_approved_at DATETIME,
    order_delivered_carrier_date DATETIME,
    order_delivered_customer_date DATETIME,
    order_estimated_delivery_date DATETIME,
    _ingested_at DATETIME,
    SHARD KEY (order_id),
    SORT KEY (_ingested_at)
);

CREATE TABLE IF NOT EXISTS raw_olist_order_payments (
    order_id VARCHAR(64),
    payment_sequential INT,
    payment_type VARCHAR(32),
    payment_installments INT,
    payment_value DECIMAL(10,2),
    _ingested_at DATETIME,
    SHARD KEY (order_id),
    SORT KEY (_ingested_at)
);

CREATE TABLE IF NOT EXISTS raw_olist_order_reviews (
    review_id VARCHAR(64),
    order_id VARCHAR(64),
    review_score INT,
    review_comment_title TEXT,
    review_comment_message TEXT,
    review_creation_date DATETIME,
    review_answer_timestamp DATETIME,
    _ingested_at DATETIME,
    SHARD KEY (order_id),
    SORT KEY (_ingested_at)
);

-- sp_trickle_olist_batch / sp_reset_olist_landing / channel fault-injection tables live in
-- singlestore/studio_schema.sql (applied for this DB by bootstrap_stack.apply_studio_schema()).
