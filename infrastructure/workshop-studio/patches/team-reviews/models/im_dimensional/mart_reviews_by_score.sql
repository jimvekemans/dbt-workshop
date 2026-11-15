{{ config(materialized="table") }}

-- Reviews team: how many customers gave one, two, ... five stars.
select review_score, count(*) as reviews
from {{ source("raw_olist", "raw_olist_order_reviews") }}
group by review_score
