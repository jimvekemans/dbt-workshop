{{
    config(
        materialized='table',
        shard_key=['order_id']
    )
}}

-- Domain B (mounted-drive team). Idempotent staging: a file delivered twice must not change the result.
with ranked as (
    select
        *,
        row_number() over (partition by review_id, order_id order by _ingested_at desc) as rn
    from {{ source('raw_olist', 'raw_olist_order_reviews') }}
    where review_id is not null
)

select
    md5(review_id) as domain_b_hkey,
    md5(order_id) as order_hkey,
    review_id,
    order_id,
    review_score,
    review_comment_title,
    review_comment_message,
    review_creation_date,
    review_answer_timestamp,
    _ingested_at
from ranked
where rn = 1
