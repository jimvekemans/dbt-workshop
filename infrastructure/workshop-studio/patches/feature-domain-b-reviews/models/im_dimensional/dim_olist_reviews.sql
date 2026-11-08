{{
    config(
        materialized='table',
        shard_key=['order_hkey']
    )
}}

select
    domain_b_hkey,
    order_hkey,
    review_id,
    order_id,
    review_score,
    case when review_score >= 4 then 'promoter' when review_score = 3 then 'passive' else 'detractor' end as nps_categorie,
    review_creation_date,
    review_answer_timestamp
from {{ ref('stg_olist_reviews') }}
