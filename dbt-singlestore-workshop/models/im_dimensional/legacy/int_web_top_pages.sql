{{ config(materialized="table") }}

-- Top landing pages per traffic source, for the marketing dashboard. Built in 2023
-- for a campaign; the author has left,
-- nobody owns it, it has no tests and no docs - but the dashboard still reads it
-- every morning.
-- The only model that reads the web team's `referrer` column.
select referrer, page, count(*) as views, count(distinct visitor_id) as bezoekers
from {{ ref("stg_web_events") }}
where event_type = 'page_view'
group by referrer, page
