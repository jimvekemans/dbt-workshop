{{
    config(
        materialized='table',
        shard_key=['order_id']
    )
}}

-- Domain B (mounted-drive team). One row per delivered review row. Deliberately NOT deduplicated:
-- the unique test in olist_domain_b.yml is what catches a file that is delivered twice.
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
from {{ source('raw_olist', 'raw_olist_order_reviews') }}
where review_id is not null
