
WITH classified_usage AS (
  SELECT
    a.id AS account_id,
    COALESCE(NULLIF(a.alias, ''), a.email) AS label,
    LOWER(a.plan_type) AS plan_type,
    CASE
      WHEN uh.window_minutes = 300 THEN 'five_hour'
      WHEN uh.window_minutes = 10080 THEN 'weekly'
      -- Keep compatibility with older store rows without window_minutes.
      WHEN uh.window_minutes IS NULL AND uh.window = 'primary' THEN 'five_hour'
      WHEN uh.window_minutes IS NULL AND uh.window = 'secondary' THEN 'weekly'
    END AS window_name,
    100.0 - uh.used_percent AS remaining_percent,
    uh.reset_at,
    uh.recorded_at,
    uh.id
  FROM accounts a
  JOIN usage_history uh ON uh.account_id = a.id
  WHERE a.status = 'active'
), latest_usage AS (
  SELECT
    account_id,
    label,
    plan_type,
    remaining_percent,
    window_name,
    reset_at,
    ROW_NUMBER() OVER (
      PARTITION BY account_id, window_name
      ORDER BY recorded_at DESC, id DESC
    ) AS rn
  FROM classified_usage
  WHERE window_name IS NOT NULL
)
SELECT
  account_id,
  label,
  plan_type,
  window_name,
  remaining_percent,
  reset_at
FROM latest_usage
WHERE rn = 1 AND window_name = 'weekly'
ORDER BY label, window_name;
