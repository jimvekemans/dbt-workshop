-- Global part of the SingleStore setup. Applied by bootstrap AFTER init_env.sql.tmpl has run for dev, val and prd,
-- because the users below get roles that only exist by then.

-- 1. Cap SingleStore memory (4 GB table memory, 5 GB total engine memory) and set the data conversion level
SET GLOBAL maximum_table_memory = 4096;
SET GLOBAL maximum_memory = 5120;
SET GLOBAL data_conversion_compatibility_level = '6.0';

-- 2. CI scratch database: every CI run builds its own schema-prefixed tables here
CREATE DATABASE IF NOT EXISTS vdp_ci;

-- 3. CI role: ALL on vdp_ci only
CREATE ROLE 'ci_dbt';
CREATE GROUP 'grp_ci_dbt';
GRANT ROLE 'ci_dbt' TO 'grp_ci_dbt';
GRANT ALL PRIVILEGES ON vdp_ci.* TO ROLE 'ci_dbt';

-- 4. Developer role: same rights as dev_dbt (dev databases only)
CREATE ROLE 'dev_developer';
CREATE GROUP 'grp_dev_developer';
GRANT ROLE 'dev_developer' TO 'grp_dev_developer';
GRANT ALL PRIVILEGES ON dev_edw_dv_rdv.* TO ROLE 'dev_developer';
GRANT ALL PRIVILEGES ON dev_edw_dv_bdv.* TO ROLE 'dev_developer';
GRANT ALL PRIVILEGES ON dev_im_dimensional.* TO ROLE 'dev_developer';
GRANT ALL PRIVILEGES ON dev_vdp_mgmt.* TO ROLE 'dev_developer';
GRANT SELECT, SHOW VIEW ON dev_landing.* TO ROLE 'dev_developer';

-- 5. Users (SingleStore RBAC: user -> group -> role -> privileges)
CREATE USER IF NOT EXISTS 'svc_ci_dbt'@'%' IDENTIFIED BY 'CiServicePwd2026!';
CREATE USER IF NOT EXISTS 'pu_dev1'@'%' IDENTIFIED BY 'WorkshopPwd2026!';
CREATE USER IF NOT EXISTS 'pu_dev2'@'%' IDENTIFIED BY 'WorkshopPwd2026!';
CREATE USER IF NOT EXISTS 'pu_dev3'@'%' IDENTIFIED BY 'WorkshopPwd2026!';
CREATE USER IF NOT EXISTS 'pu_admin'@'%' IDENTIFIED BY 'WorkshopPwd2026!';

GRANT GROUP 'grp_ci_dbt' TO 'svc_ci_dbt'@'%';
GRANT GROUP 'grp_dev_reader' TO 'svc_ci_dbt'@'%';

GRANT GROUP 'grp_dev_developer' TO 'pu_dev1'@'%';
GRANT GROUP 'grp_val_reader' TO 'pu_dev1'@'%';
GRANT GROUP 'grp_prd_reader' TO 'pu_dev1'@'%';
GRANT GROUP 'grp_dev_developer' TO 'pu_dev2'@'%';
GRANT GROUP 'grp_val_reader' TO 'pu_dev2'@'%';
GRANT GROUP 'grp_prd_reader' TO 'pu_dev2'@'%';
GRANT GROUP 'grp_dev_developer' TO 'pu_dev3'@'%';
GRANT GROUP 'grp_val_reader' TO 'pu_dev3'@'%';
GRANT GROUP 'grp_prd_reader' TO 'pu_dev3'@'%';

GRANT GROUP 'grp_dev_admin' TO 'pu_admin'@'%';
GRANT GROUP 'grp_val_admin' TO 'pu_admin'@'%';
GRANT GROUP 'grp_prd_admin' TO 'pu_admin'@'%';

FLUSH PRIVILEGES;
