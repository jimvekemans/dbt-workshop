{{ config(severity="error", tags=["critical"]) }}

-- A parcel with a delivery scan must have 'delivered' as its current status. When the
-- current status is resolved by
-- ARRIVAL order, a scan that is uploaded late (picked_up / in_transit) turns a
-- delivered parcel back into
-- "in transit": late-arriving data rewrites the timeline.
select order_id, current_status, current_status_ts, delivered_at
from {{ ref("fct_olist_deliveries") }}
where delivered_at is not null and current_status <> 'delivered'
