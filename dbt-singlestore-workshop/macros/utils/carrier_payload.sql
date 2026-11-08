{#-
    carrier_scan_ts(payload)

    Scan time of a carrier tracking payload in local business time (UTC-3, like the Olist timestamps), for both
    carrier API versions:
      v1  {"event_time": "2017-03-04 10:00:00", ...}      local time
      v2  {"eventTime": "2017-03-04T13:00:00Z", ...}      UTC, ISO-8601
    Used by source freshness (the feed, whatever version it sends) and by stg_carrier_tracking_events v2.
    The v1 parser deliberately does NOT use it: v1 only knows the v1 contract.
-#}
{% macro carrier_scan_ts(payload) -%}
coalesce(
    cast(json_extract_string({{ payload }}, 'event_time') as datetime),
    date_sub(cast(replace(replace(json_extract_string({{ payload }}, 'eventTime'), 'T', ' '), 'Z', '') as datetime), interval 3 hour)
)
{%- endmacro %}
