{%- macro post_hook_pit(object_name, fmc_pit_loading_history) -%}
    {%- set update_load_history_query -%}
        commit; -- noodzakelijk om een cross database error te vermijden
        update {{ fmc_pit_loading_history }}
        set 
            load_end_date = now(),
            success_flag = 1
        where dv_load_cycle_id = (select max(dv_load_cycle_id) from {{ fmc_pit_loading_history }} where object_name = '{{ object_name }}' )
        and object_name = '{{ object_name }}'
        and success_flag = 0
    {%- endset -%}
    {%- if should_execute() -%} {%- do run_query(update_load_history_query) -%} {%- endif -%}
{%- endmacro -%}
