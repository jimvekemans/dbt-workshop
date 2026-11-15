CREATE DATABASE IF NOT EXISTS airflow_dev CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
CREATE DATABASE IF NOT EXISTS airflow_val CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
CREATE DATABASE IF NOT EXISTS airflow_prd CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
GRANT ALL PRIVILEGES ON airflow_dev.* TO 'airflow'@'%';
GRANT ALL PRIVILEGES ON airflow_val.* TO 'airflow'@'%';
GRANT ALL PRIVILEGES ON airflow_prd.* TO 'airflow'@'%';
FLUSH PRIVILEGES;
