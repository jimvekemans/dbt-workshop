The infrastructure should contain at minimum:
- a dbt singlestore project using the local dbt 2 binary, which uses an adbc driver which is also available locally
- a singlestore instance
- two airflow 3.x setups representing dev and prod
- a local git provider with a repo containing the dbt project and the capability to run ci pipelines for the dbt project these ci pipelines can:
  - connect to singlestore and can deploy DAG changes to airflow.



The current setup should be able to do the following things:
