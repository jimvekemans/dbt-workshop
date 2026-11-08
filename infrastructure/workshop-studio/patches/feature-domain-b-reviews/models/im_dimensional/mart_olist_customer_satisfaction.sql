{{
    config(
        materialized='table'
    )
}}

-- Marketing NPS per order month: (promoters - detractors) / reviews * 100
with reviews as (
    select
        f.bestel_jaar_maand,
        r.review_score,
        r.nps_categorie
    from {{ ref('dim_olist_reviews') }} r
    inner join {{ ref('fct_olist_orders') }} f on f.order_hkey = r.order_hkey
)

select
    bestel_jaar_maand,
    count(*) as aantal_reviews,
    round(avg(review_score), 2) as gem_review_score,
    round(100 * (sum(case when nps_categorie = 'promoter' then 1 else 0 end)
               - sum(case when nps_categorie = 'detractor' then 1 else 0 end)) / count(*), 1) as nps
from reviews
group by bestel_jaar_maand
