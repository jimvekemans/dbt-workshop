{{ config(severity='error', tags=['critical']) }}

-- Every logged-in checkout ends in an order a moment later. A month in which fewer than 90% of the checkouts can be
-- matched to an order means the join between the two source teams' data is broken, not that people stopped buying.
select maand, checkouts, toegewezen_orders, toewijzing_pct
from {{ ref('mart_web_conversion') }}
where checkouts >= 50
  and toewijzing_pct < 90
