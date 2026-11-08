{{
    config(
        materialized='incremental',
        unique_key='customer_id',
        shard_key=['customer_id'],
        sort_key=['_ingested_at'],
        on_schema_change='sync_all_columns'
    )
}}

with raw_source as (
    select *
    from {{ source('raw_olist', 'raw_olist_customers') }}
    {% if is_incremental() %}
      where _ingested_at > (select coalesce(max(_ingested_at), '1970-01-01') from {{ this }})
    {% endif %}
),

deduped as (
    select
        md5(customer_id) as customer_hkey,
        customer_id,
        customer_unique_id,
        customer_zip_code_prefix,
        customer_city,
        customer_state,
        _ingested_at,
        row_number() over (partition by customer_id order by _ingested_at desc) as rn
    from raw_source
    where customer_id is not null
)

select
    customer_hkey,
    customer_id,
    customer_unique_id,
    customer_zip_code_prefix,
    customer_city,
    customer_state,
    _ingested_at
from deduped
where rn = 1
