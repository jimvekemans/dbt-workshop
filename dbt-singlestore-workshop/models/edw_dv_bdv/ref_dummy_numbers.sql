{{
  config(
    materialized='table'
  )
}}

with d as (
    select 0 as n union all select 1 union all select 2 union all select 3 union all select 4
    union all select 5 union all select 6 union all select 7 union all select 8 union all select 9
),
numbers as (
    select
        (d4.n * 10000 + d3.n * 1000 + d2.n * 100 + d1.n * 10 + d0.n + 1) :> integer as seq
    from d d0
    cross join d d1
    cross join d d2
    cross join d d3
    cross join d d4
)
select seq
from numbers
where seq <= 100000
