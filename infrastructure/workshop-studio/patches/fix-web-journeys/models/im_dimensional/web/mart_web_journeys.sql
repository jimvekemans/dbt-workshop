{{
    config(
        materialized='table'
    )
}}

-- Marketing journey analysis: per day, how many page-to-page steps visitors take and how many distinct ones.
-- Linear: each page view is paired with the NEXT page view of the same visitor (lead()), not with every later one.
-- Crawlers are left out: more than 200 page views per visitor per day is not a customer journey.
with views as (
    select visitor_id, page, event_ts, date(event_ts) as event_date
    from {{ ref('stg_web_events') }}
    where event_type = 'page_view'
      and visitor_id is not null
),

per_visitor_day as (
    select visitor_id, event_date, count(*) as views
    from views
    group by visitor_id, event_date
),

human_views as (
    select v.*
    from views v
    inner join per_visitor_day p
        on p.visitor_id = v.visitor_id
       and p.event_date = v.event_date
    where p.views <= 200
),

steps as (
    select
        visitor_id,
        event_date,
        page,
        lead(page) over (partition by visitor_id order by event_ts) as next_page
    from human_views
)

select
    event_date,
    count(*) as paden,
    count(distinct concat(page, ' > ', next_page)) as unieke_paden,
    count(distinct visitor_id) as bezoekers
from steps
where next_page is not null
group by event_date
