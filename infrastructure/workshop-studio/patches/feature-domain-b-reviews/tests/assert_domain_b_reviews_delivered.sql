{{ config(severity='error', tags=['critical']) }}

-- Fails when the Domain B source (mounted drive) has not delivered any reviews to this environment.
select 1 as reviews_not_delivered
from (select count(*) as n from {{ ref('stg_olist_reviews') }}) c
where c.n = 0
