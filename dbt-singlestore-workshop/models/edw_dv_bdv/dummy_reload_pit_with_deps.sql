{{
    config(
        materialized="incremental",
        unique_key="pit_key",
        meta={"reload_strategy": {"type": "pit", "column": "pit_key"}},
    )
}}

-- The hooks in dummy_reload.yml write to dummy_load_cycles: build that table first.
-- depends_on: {{ ref("dummy_load_cycles") }}
select
    d.datum_id :> integer(8) as pit_key,
    h.last_loaded_date :> date as last_loaded_date,
    current_timestamp() :> datetime(6) as load_timestamp
from {{ ref("datum") }} d
cross join {{ ref("dummy_loading_history") }} h
{% if is_incremental() %}
    where d.datum_id > (select coalesce(max(pit_key), 0) from {{ this }})
{% endif %}
