{{
  config(
    materialized='incremental',
    unique_key='hkey_record',
    meta={'reload_strategy': {'type': 'hkey_mod', 'column': 'hkey_record'}}
  )
}}

with raw_data as (
    select
        seq :> integer as id,
        md5(concat('salt_', seq)) :> varchar(32) as hkey_record
    from {{ ref('ref_dummy_numbers') }}
    where seq <= 100000
)
select
    id,
    hkey_record
from raw_data
