WITH u AS (
    SELECT DISTINCT ON (rid) c_ip, sc_status, cs_method,
        regexp_extract(cs_uri,'^[^?]*') p, substr(cs_user_agent,1,60) ua, sc_bytes
    FROM access_mp2
    WHERE time_local LIKE '30/Sep/2026:19:%'
        AND substr(time_local,13,8) BETWEEN '19:29:20' AND '19:32:59'
        AND domain LIKE '%www.tos.cz'
)
SELECT 'ip' k, c_ip v, count(*) n,
    round(sum(TRY_CAST(sc_bytes AS BIGINT))/1e6,1) mb
FROM u GROUP BY 2 QUALIFY row_number() OVER (ORDER BY count(*) DESC)<=10
UNION ALL SELECT 'path', cs_method||' '||substr(p,1,60), count(*),
    round(sum(TRY_CAST(sc_bytes AS BIGINT))/1e6,1)
FROM u GROUP BY 2 QUALIFY row_number() OVER (ORDER BY count(*) DESC)<=10
UNION ALL SELECT 'status', sc_status, count(*),
    round(sum(TRY_CAST(sc_bytes AS BIGINT))/1e6,1) FROM u GROUP BY 2
UNION ALL SELECT 'ua', ua, count(*), NULL
FROM u GROUP BY 2 QUALIFY row_number() OVER (ORDER BY count(*) DESC)<=5
UNION ALL SELECT 'uniq_ip', CAST(count(DISTINCT c_ip) AS VARCHAR), count(*), NULL
FROM u ORDER BY 1, 3 DESC
