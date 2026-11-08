CREATE DATABASE IF NOT EXISTS airflow_dev CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
CREATE DATABASE IF NOT EXISTS airflow_prod CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
GRANT ALL PRIVILEGES ON airflow_dev.* TO 'airflow'@'%';
GRANT ALL PRIVILEGES ON airflow_prod.* TO 'airflow'@'%';
FLUSH PRIVILEGES;
