{{
  config(
    materialized='table'
  )
}}

select
    cast('2026-01-01' as date) as last_loaded_date,
    'dummy_reload_pipeline' :> varchar(64) as pipeline_name
