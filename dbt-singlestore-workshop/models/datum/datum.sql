with
    datespine as (
        {{
            dbt_utils.date_spine(
                datepart="day",
                start_date="cast('1900-01-01' as date)",
                end_date="cast('2101-01-01' as date)",
            )
        }}
    ),
    datum as (
        select
            date_format(date_day, '%Y%m%d') as x_datum_dakey,
            date_day as datum_dt,
            date_day as datum,
            date_format(date_day, '%d-%m-%Y') as datum_lbl1,
            date_format(date_day, '%d/%m/%Y') as datum_lbl2,
            date_format(date_day, '%d-%m') as datum_lbl3,
            date_format(date_day, '%d/%m') as datum_lbl4,
            case
                dayname(date_day)
                when 'Monday'
                then 'maandag'
                when 'Tuesday'
                then 'dinsdag'
                when 'Wednesday'
                then 'woensdag'
                when 'Thursday'
                then 'donderdag'
                when 'Friday'
                then 'vrijdag'
                when 'Saturday'
                then 'zaterdag'
                when 'Sunday'
                then 'zondag'
            end as dag_naam,
            case
                date_format(date_day, '%a')
                when 'Mon'
                then 'ma'
                when 'Tue'
                then 'di'
                when 'Wed'
                then 'wo'
                when 'Thu'
                then 'do'
                when 'Fri'
                then 'vr'
                when 'Sat'
                then 'za'
                when 'Sun'
                then 'zo'
            end as dag_lbl1,
            (weekday(date_day) + 1) as dag_in_week_nr,  -- Day of the week (0=Sunday to 6=Saturday)
            to_number(date_format(date_day, '%d')) as dag_in_maand_nr,
            dayofyear(date_day) - case
                quarter(date_day)
                when 1
                then 0
                when 2
                then dayofyear(concat(year(date_day), '0331'))
                when 3
                then dayofyear(concat(year(date_day), '0630'))
                when 4
                then dayofyear(concat(year(date_day), '0930'))
            end as dag_in_kwartaal_nr,
            dayofyear(date_day) - case
                when date_format(date_day, '%m%d') > '0630'
                then dayofyear(concat(year(date_day), '0630'))
                else 0
            end as dag_in_semester_nr,
            dayofyear(date_day) as dag_in_jaar_nr,
            case
                when weekday(date_day) = 0 then 'J' else 'N'
            end as is_begin_van_week_vlag,
            case
                when weekday(date_day) = 6 then 'J' else 'N'
            end as is_einde_van_week_vlag,
            case
                when date_format(date_day, '%d') = '01' then 'J' else 'N'
            end as is_begin_van_maand_vlag,
            case
                when last_day(date_day) = date_day then 'J' else 'N'
            end as is_einde_van_maand_vlag,
            case
                when date_format(date_day, '%m%d') in ('0101', '0401', '0701', '1001')
                then 'J'
                else 'N'
            end as is_begin_van_kwartaal_vlag,
            case
                when date_format(date_day, '%m%d') in ('0331', '0630', '0930', '1231')
                then 'J'
                else 'N'
            end as is_einde_van_kwartaal_vlag,
            case
                when date_format(date_day, '%m%d') in ('0101', '0701') then 'J' else 'N'
            end as is_begin_van_semester_vlag,
            case
                when date_format(date_day, '%m%d') in ('0630', '1231') then 'J' else 'N'
            end as is_einde_van_semester_vlag,
            case
                when date_format(date_day, '%m%d') = '0101' then 'J' else 'N'
            end as is_begin_van_jaar_vlag,
            case
                when date_format(date_day, '%m%d') = '1231' then 'J' else 'N'
            end as is_einde_van_jaar_vlag,
            to_number(date_format(date_day, '%Y%v')) as x_week_dakey,
            concat(year(date_day), ' w', date_format(date_day, '%v')) as jaar_week_lbl1,
            concat(
                year(date_day), ' week ', date_format(date_day, '%v')
            ) as jaar_week_lbl2,
            to_number(date_format(date_day, '%v')) as week_in_jaar_nr,
            concat('w', date_format(date_day, '%v')) as week_in_jaar_lbl1,
            concat('week ', date_format(date_day, '%v')) as week_in_jaar_lbl2,
            to_number(
                date_format(
                    date_add(date_day, interval - weekday(date_day) day), '%Y%m%d'
                )
            ) as x_begin_week_datum_dakey,
            to_number(
                date_format(
                    date_add(date_day, interval 6 - weekday(date_day) day), '%Y%m%d'
                )
            ) as x_einde_week_datum_dakey,
            to_number(date_format(date_day, '%Y%m')) as x_maand_dakey,
            concat(
                year(date_day), ' / ', date_format(date_day, '%m')
            ) as jaar_maand_lbl1,
            concat(year(date_day), 'M', date_format(date_day, '%m')) as jaar_maand_lbl2,
            concat(
                case
                    monthname(date_day)
                    when 'January'
                    then 'januari'
                    when 'February'
                    then 'februari'
                    when 'March'
                    then 'maart'
                    when 'April'
                    then 'april'
                    when 'May'
                    then 'mei'
                    when 'June'
                    then 'juni'
                    when 'July'
                    then 'juli'
                    when 'August'
                    then 'augustus'
                    when 'September'
                    then 'september'
                    when 'October'
                    then 'oktober'
                    when 'November'
                    then 'november'
                    when 'December'
                    then 'december'
                end,
                ' ',
                date_format(date_day, '%Y')
            ) as jaar_maand_lbl3,
            date_format(date_day, '%Y%m') as jaar_maand_lbl4,
            concat(year(date_day), '/', date_format(date_day, '%m')) as jaar_maand_lbl5,
            concat(year(date_day), '-', date_format(date_day, '%m')) as jaar_maand_lbl6,
            to_number(
                trim(leading '0' from date_format(date_day, '%m'))
            ) as maand_in_jaar_nr,
            date_format(date_day, '%m') as maand_in_jaar_lbl1,
            case
                monthname(date_day)
                when 'January'
                then 'januari'
                when 'February'
                then 'februari'
                when 'March'
                then 'maart'
                when 'April'
                then 'april'
                when 'May'
                then 'mei'
                when 'June'
                then 'juni'
                when 'July'
                then 'juli'
                when 'August'
                then 'augustus'
                when 'September'
                then 'september'
                when 'October'
                then 'oktober'
                when 'November'
                then 'november'
                when 'December'
                then 'december'
            end as maand_naam,
            case
                date_format(date_day, '%M')
                when 'January'
                then 'jan'
                when 'February'
                then 'feb'
                when 'March'
                then 'mrt'
                when 'April'
                then 'apr'
                when 'May'
                then 'mei'
                when 'June'
                then 'jun'
                when 'July'
                then 'jul'
                when 'August'
                then 'aug'
                when 'September'
                then 'sep'
                when 'October'
                then 'okt'
                when 'November'
                then 'nov'
                when 'December'
                then 'dec'
            end as maand_lbl1,
            trunc(date_day, 'MM') as begin_maand_dt,
            timestamp(last_day(date_day)) as einde_maand_dt,
            date_format(trunc(date_day, 'MM'), '%Y%m%d') as x_begin_maand_datum_dakey,
            date_format(last_day(date_day), '%Y%m%d') as x_einde_maand_datum_dakey,
            date_format(
                date_add(date_day, interval -1 month), '%Y%m'
            ) as x_vorige_maand_maand_dakey,
            date_format(
                date_add(date_day, interval 1 month), '%Y%m'
            ) as x_volgende_maand_maand_dakey,
            date_format(last_day(date_day), '%d') as aantal_dagen_in_maand,
            date_format(
                date_add(date_day, interval -1 year), '%Y%m'
            ) as x_vorig_jaar_maand_dakey,
            date_format(
                date_add(date_day, interval 1 year), '%Y%m'
            ) as x_volgend_jaar_maand_dakey,
            concat(date_format(date_day, '%Y'), quarter(date_day)) as x_kwartaal_dakey,
            concat(year(date_day), ' Q', quarter(date_day)) as jaar_kwartaal_lbl1,
            concat(year(date_day), ' / ', quarter(date_day)) as jaar_kwartaal_lbl2,
            concat(year(date_day), '/', quarter(date_day)) as jaar_kwartaal_lbl3,
            concat(
                quarter(date_day), 'e kwartaal ', year(date_day)
            ) as jaar_kwartaal_lbl4,
            concat(year(date_day), '/Q', quarter(date_day)) as jaar_kwartaal_lbl5,
            quarter(date_day) as kwartaal_in_jaar_nr,
            concat('Q', quarter(date_day)) as kwartaal_in_jaar_lbl1,
            concat('Kwartaal ', quarter(date_day)) as kwartaal_in_jaar_lbl2,
            concat(quarter(date_day), 'e kwartaal') as kwartaal_in_jaar_lbl3,
            trunc(date_day, 'Q') begin_kwartaal_dt,
            case
                when quarter(date_day) = 1
                then timestamp(concat(year(date_day), '0331'))
                when quarter(date_day) = 2
                then timestamp(concat(year(date_day), '0630'))
                when quarter(date_day) = 3
                then timestamp(concat(year(date_day), '0930'))
                when quarter(date_day) = 4
                then timestamp(concat(year(date_day), '1231'))
            end as einde_kwartaal_dt,
            date_format(trunc(date_day, 'Q'), '%Y%m%d') as x_begin_kwartaal_datum_dakey,
            case
                when quarter(date_day) = 1
                then concat(year(date_day), '0331')
                when quarter(date_day) = 2
                then concat(year(date_day), '0630')
                when quarter(date_day) = 3
                then concat(year(date_day), '0930')
                when quarter(date_day) = 4
                then concat(year(date_day), '1231')
            end as x_einde_kwartaal_datum_dakey,
            concat(
                year(date_day), quarter(timestampadd(month, -3, date_day))
            ) as x_vorig_kwartaal_kwartaal_dakey,
            concat(
                year(date_day), quarter(timestampadd(month, 3, date_day))
            ) as x_volgend_kwartaal_kwartaal_dakey,
            date_format(
                timestampadd(month, 3, date_day), '%Y%m'
            ) as x_volgend_kwartaal_maand_dakey,
            concat(
                year(date_day) - 1, quarter(date_day)
            ) as x_vorig_jaar_kwartaal_dakey,
            concat(
                year(date_day) + 1, quarter(date_day)
            ) as x_volgend_jaar_kwartaal_dakey,
            case
                when quarter(date_day) in (3, 4)
                then 92
                when quarter(date_day) = 2
                then 91
                when
                    quarter(date_day) = 1
                    and to_number(dayofyear(concat(year(date_day), '1231'))) = 366
                then 91
                else 90
            end as aantal_dagen_in_kwartaal,
            concat(
                date_format(date_day, '%Y'),
                case when quarter(date_day) > 2 then 2 else 1 end
            ) as x_semester_dakey,
            concat(
                year(date_day), ' S', case when quarter(date_day) > 2 then 2 else 1 end
            ) as jaar_semester_lbl1,
            concat(
                year(date_day), ' / ', case when quarter(date_day) > 2 then 2 else 1 end
            ) as jaar_semester_lbl2,
            concat(
                year(date_day), '/', case when quarter(date_day) > 2 then 2 else 1 end
            ) as jaar_semester_lbl3,
            concat(
                case when quarter(date_day) > 2 then 2 else 1 end,
                'e semester ',
                year(date_day)
            ) as jaar_semester_lbl4,
            case when quarter(date_day) > 2 then 2 else 1 end as semester_in_jaar_nr,
            case
                when quarter(date_day) > 2 then 'S2' else 'S1'
            end as semester_in_jaar_lbl1,
            concat(
                'Semester ', case when quarter(date_day) > 2 then '2' else '1' end
            ) as semester_in_jaar_lbl2,
            concat(
                case when quarter(date_day) > 2 then '2' else '1' end, 'e semester'
            ) as semester_in_jaar_lbl3,
            year(date_day) as x_jaar_dakey,
            year(date_day) as jaar,
            date_format(
                date_add(date_day, interval -1 year), '%Y%m'
            ) as x_vorig_jaar_dakey,
            year(date_day) as jaar_lbl1,
            trunc(date_day, 'YYYY') as begin_jaar_dt,
            cast(trunc(date_day, 'YYYY') as date) as x_begin_jaar_datum_dakey,
            timestamp(date_format(date_day, '%Y-12-31')) as einde_jaar_dt,
            date_format(date_day, '%Y-12-31') as x_einde_jaar_datum_dakey,
            dayofyear(concat(year(date_day), '1231')) as aantal_dagen_in_jaar
        from datespine
    ),
    dummy1 as (
        select
            -1 as x_datum_dakey,
            null as datum_dt,
            null as datum,
            '<ONBEKEND>' as datum_lbl1,
            null as datum_lbl2,
            null as datum_lbl3,
            null as datum_lbl4,
            null as dag_naam,
            null as dag_lbl1,
            null as dag_in_week_nr,
            null as dag_in_maand_nr,
            null as dag_in_kwartaal_nr,
            null as dag_in_semester_nr,
            null as dag_in_jaar_nr,
            null as is_begin_van_week_vlag,
            null as is_einde_van_week_vlag,
            null as is_begin_van_maand_vlag,
            null as is_einde_van_maand_vlag,
            null as is_begin_van_kwartaal_vlag,
            null as is_einde_van_kwartaal_vlag,
            null as is_begin_van_semester_vlag,
            null as is_einde_van_semester_vlag,
            null as is_begin_van_jaar_vlag,
            null as is_einde_van_jaar_vlag,
            null as x_week_dakey,
            null as jaar_week_lbl1,
            null as jaar_week_lbl2,
            null as week_in_jaar_nr,
            null as week_in_jaar_lbl1,
            null as week_in_jaar_lbl2,
            null as x_begin_week_datum_dakey,
            null as x_einde_week_datum_dakey,
            null as x_maand_dakey,
            null as jaar_maand_lbl1,
            null as jaar_maand_lbl2,
            null as jaar_maand_lbl3,
            null as jaar_maand_lbl4,
            null as jaar_maand_lbl5,
            null as jaar_maand_lbl6,
            null as maand_in_jaar_nr,
            null as maand_in_jaar_lbl1,
            null as maand_naam,
            null as maand_lbl1,
            null as begin_maand_dt,
            null as einde_maand_dt,
            null as x_begin_maand_datum_dakey,
            null as x_einde_maand_datum_dakey,
            null as x_vorige_maand_maand_dakey,
            null as x_volgende_maand_maand_dakey,
            null as aantal_dagen_in_maand,
            null as x_vorig_jaar_maand_dakey,
            null as x_volgend_jaar_maand_dakey,
            null as x_kwartaal_dakey,
            null as jaar_kwartaal_lbl1,
            null as jaar_kwartaal_lbl2,
            null as jaar_kwartaal_lbl3,
            null as jaar_kwartaal_lbl4,
            null as jaar_kwartaal_lbl5,
            null as kwartaal_in_jaar_nr,
            null as kwartaal_in_jaar_lbl1,
            null as kwartaal_in_jaar_lbl2,
            null as kwartaal_in_jaar_lbl3,
            null as begin_kwartaal_dt,
            null as einde_kwartaal_dt,
            null as x_begin_kwartaal_datum_dakey,
            null as x_einde_kwartaal_datum_dakey,
            null as x_vorig_kwartaal_kwartaal_dakey,
            null as x_volgend_kwartaal_kwartaal_dakey,
            null as x_volgend_kwartaal_maand_dakey,
            null as x_vorig_jaar_kwartaal_dakey,
            null as x_volgend_jaar_kwartaal_dakey,
            null as aantal_dagen_in_kwartaal,
            null as x_semester_dakey,
            null as jaar_semester_lbl1,
            null as jaar_semester_lbl2,
            null as jaar_semester_lbl3,
            null as jaar_semester_lbl4,
            null as semester_in_jaar_nr,
            null as semester_in_jaar_lbl1,
            null as semester_in_jaar_lbl2,
            null as semester_in_jaar_lbl3,
            null as x_jaar_dakey,
            null as jaar,
            null as x_vorig_jaar_dakey,
            null as jaar_lbl1,
            null as begin_jaar_dt,
            null as x_begin_jaar_datum_dakey,
            null as einde_jaar_dt,
            null as x_einde_jaar_datum_dakey,
            null as aantal_dagen_in_jaar
    ),
    dummy2 as (
        select
            -2 as x_datum_dakey,
            null as datum_dt,
            null as datum,
            '<NVT>' as datum_lbl1,
            null as datum_lbl2,
            null as datum_lbl3,
            null as datum_lbl4,
            null as dag_naam,
            null as dag_lbl1,
            null as dag_in_week_nr,
            null as dag_in_maand_nr,
            null as dag_in_kwartaal_nr,
            null as dag_in_semester_nr,
            null as dag_in_jaar_nr,
            null as is_begin_van_week_vlag,
            null as is_einde_van_week_vlag,
            null as is_begin_van_maand_vlag,
            null as is_einde_van_maand_vlag,
            null as is_begin_van_kwartaal_vlag,
            null as is_einde_van_kwartaal_vlag,
            null as is_begin_van_semester_vlag,
            null as is_einde_van_semester_vlag,
            null as is_begin_van_jaar_vlag,
            null as is_einde_van_jaar_vlag,
            null as x_week_dakey,
            null as jaar_week_lbl1,
            null as jaar_week_lbl2,
            null as week_in_jaar_nr,
            null as week_in_jaar_lbl1,
            null as week_in_jaar_lbl2,
            null as x_begin_week_datum_dakey,
            null as x_einde_week_datum_dakey,
            null as x_maand_dakey,
            null as jaar_maand_lbl1,
            null as jaar_maand_lbl2,
            null as jaar_maand_lbl3,
            null as jaar_maand_lbl4,
            null as jaar_maand_lbl5,
            null as jaar_maand_lbl6,
            null as maand_in_jaar_nr,
            null as maand_in_jaar_lbl1,
            null as maand_naam,
            null as maand_lbl1,
            null as begin_maand_dt,
            null as einde_maand_dt,
            null as x_begin_maand_datum_dakey,
            null as x_einde_maand_datum_dakey,
            null as x_vorige_maand_maand_dakey,
            null as x_volgende_maand_maand_dakey,
            null as aantal_dagen_in_maand,
            null as x_vorig_jaar_maand_dakey,
            null as x_volgend_jaar_maand_dakey,
            null as x_kwartaal_dakey,
            null as jaar_kwartaal_lbl1,
            null as jaar_kwartaal_lbl2,
            null as jaar_kwartaal_lbl3,
            null as jaar_kwartaal_lbl4,
            null as jaar_kwartaal_lbl5,
            null as kwartaal_in_jaar_nr,
            null as kwartaal_in_jaar_lbl1,
            null as kwartaal_in_jaar_lbl2,
            null as kwartaal_in_jaar_lbl3,
            null as begin_kwartaal_dt,
            null as einde_kwartaal_dt,
            null as x_begin_kwartaal_datum_dakey,
            null as x_einde_kwartaal_datum_dakey,
            null as x_vorig_kwartaal_kwartaal_dakey,
            null as x_volgend_kwartaal_kwartaal_dakey,
            null as x_volgend_kwartaal_maand_dakey,
            null as x_vorig_jaar_kwartaal_dakey,
            null as x_volgend_jaar_kwartaal_dakey,
            null as aantal_dagen_in_kwartaal,
            null as x_semester_dakey,
            null as jaar_semester_lbl1,
            null as jaar_semester_lbl2,
            null as jaar_semester_lbl3,
            null as jaar_semester_lbl4,
            null as semester_in_jaar_nr,
            null as semester_in_jaar_lbl1,
            null as semester_in_jaar_lbl2,
            null as semester_in_jaar_lbl3,
            null as x_jaar_dakey,
            null as jaar,
            null as x_vorig_jaar_dakey,
            null as jaar_lbl1,
            null as begin_jaar_dt,
            null as x_begin_jaar_datum_dakey,
            null as einde_jaar_dt,
            null as x_einde_jaar_datum_dakey,
            null as aantal_dagen_in_jaar
    ),
    dummy3 as (
        select
            -3 as x_datum_dakey,
            null as datum_dt,
            null as datum,
            '<ONBEKENDE WAARDE>' as datum_lbl1,
            null as datum_lbl2,
            null as datum_lbl3,
            null as datum_lbl4,
            null as dag_naam,
            null as dag_lbl1,
            null as dag_in_week_nr,
            null as dag_in_maand_nr,
            null as dag_in_kwartaal_nr,
            null as dag_in_semester_nr,
            null as dag_in_jaar_nr,
            null as is_begin_van_week_vlag,
            null as is_einde_van_week_vlag,
            null as is_begin_van_maand_vlag,
            null as is_einde_van_maand_vlag,
            null as is_begin_van_kwartaal_vlag,
            null as is_einde_van_kwartaal_vlag,
            null as is_begin_van_semester_vlag,
            null as is_einde_van_semester_vlag,
            null as is_begin_van_jaar_vlag,
            null as is_einde_van_jaar_vlag,
            null as x_week_dakey,
            null as jaar_week_lbl1,
            null as jaar_week_lbl2,
            null as week_in_jaar_nr,
            null as week_in_jaar_lbl1,
            null as week_in_jaar_lbl2,
            null as x_begin_week_datum_dakey,
            null as x_einde_week_datum_dakey,
            null as x_maand_dakey,
            null as jaar_maand_lbl1,
            null as jaar_maand_lbl2,
            null as jaar_maand_lbl3,
            null as jaar_maand_lbl4,
            null as jaar_maand_lbl5,
            null as jaar_maand_lbl6,
            null as maand_in_jaar_nr,
            null as maand_in_jaar_lbl1,
            null as maand_naam,
            null as maand_lbl1,
            null as begin_maand_dt,
            null as einde_maand_dt,
            null as x_begin_maand_datum_dakey,
            null as x_einde_maand_datum_dakey,
            null as x_vorige_maand_maand_dakey,
            null as x_volgende_maand_maand_dakey,
            null as aantal_dagen_in_maand,
            null as x_vorig_jaar_maand_dakey,
            null as x_volgend_jaar_maand_dakey,
            null as x_kwartaal_dakey,
            null as jaar_kwartaal_lbl1,
            null as jaar_kwartaal_lbl2,
            null as jaar_kwartaal_lbl3,
            null as jaar_kwartaal_lbl4,
            null as jaar_kwartaal_lbl5,
            null as kwartaal_in_jaar_nr,
            null as kwartaal_in_jaar_lbl1,
            null as kwartaal_in_jaar_lbl2,
            null as kwartaal_in_jaar_lbl3,
            null as begin_kwartaal_dt,
            null as einde_kwartaal_dt,
            null as x_begin_kwartaal_datum_dakey,
            null as x_einde_kwartaal_datum_dakey,
            null as x_vorig_kwartaal_kwartaal_dakey,
            null as x_volgend_kwartaal_kwartaal_dakey,
            null as x_volgend_kwartaal_maand_dakey,
            null as x_vorig_jaar_kwartaal_dakey,
            null as x_volgend_jaar_kwartaal_dakey,
            null as aantal_dagen_in_kwartaal,
            null as x_semester_dakey,
            null as jaar_semester_lbl1,
            null as jaar_semester_lbl2,
            null as jaar_semester_lbl3,
            null as jaar_semester_lbl4,
            null as semester_in_jaar_nr,
            null as semester_in_jaar_lbl1,
            null as semester_in_jaar_lbl2,
            null as semester_in_jaar_lbl3,
            null as x_jaar_dakey,
            null as jaar,
            null as x_vorig_jaar_dakey,
            null as jaar_lbl1,
            null as begin_jaar_dt,
            null as x_begin_jaar_datum_dakey,
            null as einde_jaar_dt,
            null as x_einde_jaar_datum_dakey,
            null as aantal_dagen_in_jaar
    ),
    semifinal as (
        select *
        from datum
        union all
        select *
        from dummy1
        union all
        select *
        from dummy2
        union all
        select *
        from dummy3
    ),
    final as (
        select
            x_datum_dakey :> integer(8) as x_datum_dakey,
            x_datum_dakey :> integer(8) as datum_id,
            datum_dt :> datetime(6) as datum_dt,
            datum :> date as datum,
            datum_lbl1 :> varchar(20) as datum_lbl1,
            datum_lbl2 :> varchar(10) as datum_lbl2,
            datum_lbl3 :> varchar(10) as datum_lbl3,
            datum_lbl4 :> varchar(10) as datum_lbl4,
            dag_naam :> varchar(10) as dag_naam,
            dag_lbl1 :> varchar(5) as dag_lbl1,
            dag_in_week_nr :> integer(1) as dag_in_week_nr,
            dag_in_maand_nr :> integer(2) as dag_in_maand_nr,
            dag_in_kwartaal_nr :> integer(2) as dag_in_kwartaal_nr,
            dag_in_semester_nr :> integer(3) as dag_in_semester_nr,
            dag_in_jaar_nr :> integer(3) as dag_in_jaar_nr,
            is_begin_van_week_vlag :> varchar(5) as is_begin_van_week_vlag,
            is_einde_van_week_vlag :> varchar(5) as is_einde_van_week_vlag,
            is_begin_van_maand_vlag :> varchar(5) as is_begin_van_maand_vlag,
            is_einde_van_maand_vlag :> varchar(5) as is_einde_van_maand_vlag,
            is_begin_van_kwartaal_vlag :> varchar(5) as is_begin_van_kwartaal_vlag,
            is_einde_van_kwartaal_vlag :> varchar(5) as is_einde_van_kwartaal_vlag,
            is_begin_van_semester_vlag :> varchar(5) as is_begin_van_semester_vlag,
            is_einde_van_semester_vlag :> varchar(5) as is_einde_van_semester_vlag,
            is_begin_van_jaar_vlag :> varchar(5) as is_begin_van_jaar_vlag,
            is_einde_van_jaar_vlag :> varchar(5) as is_einde_van_jaar_vlag,
            x_week_dakey :> integer(5) as x_week_dakey,
            jaar_week_lbl1 :> varchar(8) as jaar_week_lbl1,
            jaar_week_lbl2 :> varchar(12) as jaar_week_lbl2,
            week_in_jaar_nr :> integer(2) as week_in_jaar_nr,
            week_in_jaar_lbl1 :> varchar(3) as week_in_jaar_lbl1,
            week_in_jaar_lbl2 :> varchar(7) as week_in_jaar_lbl2,
            x_begin_week_datum_dakey :> integer(8) as x_begin_week_datum_dakey,
            x_einde_week_datum_dakey :> integer(8) as x_einde_week_datum_dakey,
            x_maand_dakey :> integer(6) as x_maand_dakey,
            jaar_maand_lbl1 :> varchar(9) as jaar_maand_lbl1,
            jaar_maand_lbl2 :> varchar(7) as jaar_maand_lbl2,
            jaar_maand_lbl3 :> varchar(14) as jaar_maand_lbl3,
            jaar_maand_lbl4 :> varchar(6) as jaar_maand_lbl4,
            jaar_maand_lbl5 :> varchar(7) as jaar_maand_lbl5,
            jaar_maand_lbl6 :> varchar(7) as jaar_maand_lbl6,
            maand_in_jaar_nr :> integer(2) as maand_in_jaar_nr,
            maand_in_jaar_lbl1 :> varchar(5) as maand_in_jaar_lbl1,
            maand_naam :> varchar(9) as maand_naam,
            maand_lbl1 :> varchar(5) as maand_lbl1,
            begin_maand_dt :> datetime(6) as begin_maand_dt,
            einde_maand_dt :> datetime(6) as einde_maand_dt,
            x_begin_maand_datum_dakey :> integer(8) as x_begin_maand_datum_dakey,
            x_einde_maand_datum_dakey :> integer(8) as x_einde_maand_datum_dakey,
            x_vorige_maand_maand_dakey :> integer(6) as x_vorige_maand_maand_dakey,
            x_volgende_maand_maand_dakey :> integer(6) as x_volgende_maand_maand_dakey,
            aantal_dagen_in_maand :> integer(2) as aantal_dagen_in_maand,
            x_vorig_jaar_maand_dakey :> integer(6) as x_vorig_jaar_maand_dakey,
            x_volgend_jaar_maand_dakey :> integer(6) as x_volgend_jaar_maand_dakey,
            x_kwartaal_dakey :> integer(5) as x_kwartaal_dakey,
            jaar_kwartaal_lbl1 :> varchar(7) as jaar_kwartaal_lbl1,
            jaar_kwartaal_lbl2 :> varchar(8) as jaar_kwartaal_lbl2,
            jaar_kwartaal_lbl3 :> varchar(6) as jaar_kwartaal_lbl3,
            jaar_kwartaal_lbl4 :> varchar(16) as jaar_kwartaal_lbl4,
            jaar_kwartaal_lbl5 :> varchar(7) as jaar_kwartaal_lbl5,
            kwartaal_in_jaar_nr :> integer(1) as kwartaal_in_jaar_nr,
            kwartaal_in_jaar_lbl1 :> varchar(5) as kwartaal_in_jaar_lbl1,
            kwartaal_in_jaar_lbl2 :> varchar(10) as kwartaal_in_jaar_lbl2,
            kwartaal_in_jaar_lbl3 :> varchar(11) as kwartaal_in_jaar_lbl3,
            begin_kwartaal_dt :> datetime(6) as begin_kwartaal_dt,
            einde_kwartaal_dt :> datetime(6) as einde_kwartaal_dt,
            x_begin_kwartaal_datum_dakey :> integer(8) as x_begin_kwartaal_datum_dakey,
            x_einde_kwartaal_datum_dakey :> integer(8) as x_einde_kwartaal_datum_dakey,
            x_vorig_kwartaal_kwartaal_dakey
            :> integer(5) as x_vorig_kwartaal_kwartaal_dakey,
            x_volgend_kwartaal_kwartaal_dakey
            :> integer(5) as x_volgend_kwartaal_kwartaal_dakey,
            x_volgend_kwartaal_maand_dakey
            :> integer(6) as x_volgend_kwartaal_maand_dakey,
            x_vorig_jaar_kwartaal_dakey :> integer(5) as x_vorig_jaar_kwartaal_dakey,
            x_volgend_jaar_kwartaal_dakey
            :> integer(5) as x_volgend_jaar_kwartaal_dakey,
            aantal_dagen_in_kwartaal :> integer(2) as aantal_dagen_in_kwartaal,
            x_semester_dakey :> integer(5) as x_semester_dakey,
            jaar_semester_lbl1 :> varchar(7) as jaar_semester_lbl1,
            jaar_semester_lbl2 :> varchar(8) as jaar_semester_lbl2,
            jaar_semester_lbl3 :> varchar(6) as jaar_semester_lbl3,
            jaar_semester_lbl4 :> varchar(16) as jaar_semester_lbl4,
            semester_in_jaar_nr :> integer(1) as semester_in_jaar_nr,
            semester_in_jaar_lbl1 :> varchar(5) as semester_in_jaar_lbl1,
            semester_in_jaar_lbl2 :> varchar(10) as semester_in_jaar_lbl2,
            semester_in_jaar_lbl3 :> varchar(11) as semester_in_jaar_lbl3,
            x_jaar_dakey :> integer(4) as x_jaar_dakey,
            jaar :> integer(4) as jaar,
            x_vorig_jaar_dakey :> integer(6) as x_vorig_jaar_dakey,
            jaar_lbl1 :> varchar(5) as jaar_lbl1,
            begin_jaar_dt :> datetime(6) as begin_jaar_dt,
            x_begin_jaar_datum_dakey :> integer(8) as x_begin_jaar_datum_dakey,
            einde_jaar_dt :> datetime(6) as einde_jaar_dt,
            x_einde_jaar_datum_dakey :> integer(8) as x_einde_jaar_datum_dakey,
            aantal_dagen_in_jaar :> integer(3) as aantal_dagen_in_jaar
        from semifinal
    )
select *
from final
