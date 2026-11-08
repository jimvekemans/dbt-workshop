-- BIT-specific: converts UPPER_SNAKE_CASE code values to sentence case descriptions.
-- initcap singlestore function capitalizes each word, therefore we went for concat
{% macro format_sentence_case(column_name) %}
    case
        when {{ column_name }} in ('~NL~', '~UN~')
        then {{ column_name }}
        else     
            CONCAT(
                UPPER(LEFT(REPLACE(LOWER({{ column_name }}), '_', ' '), 1)),
                SUBSTRING(REPLACE(LOWER({{ column_name }}), '_', ' '), 2)
            )
    end :> varchar(150)
{% endmacro %}