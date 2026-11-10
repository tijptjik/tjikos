-- Seek the latest qualifying weekly row for each active account. Ranking the
-- entire history sorted every old sample on each bar refresh.
SELECT
  a.id AS account_id,
  COALESCE(NULLIF(a.alias, ''), a.email) AS label,
  LOWER(a.plan_type) AS plan_type,
  'weekly' AS window_name,
  100.0 - uh.used_percent AS remaining_percent,
  uh.reset_at
FROM accounts a
JOIN usage_history uh ON uh.id = (
  SELECT h.id
  FROM usage_history h
  WHERE h.account_id = a.id
    AND (h.window_minutes = 10080
      OR (h.window_minutes IS NULL AND h.window = 'secondary'))
  ORDER BY h.recorded_at DESC, h.id DESC
  LIMIT 1
)
WHERE a.status = 'active'
ORDER BY label, window_name;
