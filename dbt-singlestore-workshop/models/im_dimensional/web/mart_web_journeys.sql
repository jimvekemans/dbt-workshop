{{
    config(
        materialized='table'
    )
}}

-- Marketing journey analysis: per day, how many page-to-page paths visitors take and how many distinct ones.
-- Written the naive way: every page view is joined to EVERY later page view of the same visitor (a non-equi self-join),
-- then the distinct paths are counted. Fine while a visitor has a handful of hits - quadratic in the hits per visitor.
-- One crawler with tens of thousands of hits on one cookie turns this into hundreds of millions of rows and SingleStore
-- runs out of memory (scenario T3).
with views as (
    select visitor_id, page, event_ts
    from {{ ref('stg_web_events') }}
    where event_type = 'page_view'
      and visitor_id is not null
)

select
    date(a.event_ts) as event_date,
    count(*) as paden,
    count(distinct concat(a.page, ' > ', b.page)) as unieke_paden,
    count(distinct a.visitor_id) as bezoekers
from views a
inner join views b
    on b.visitor_id = a.visitor_id
   and b.event_ts > a.event_ts
group by date(a.event_ts)
