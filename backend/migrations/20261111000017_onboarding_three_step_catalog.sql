-- Onboarding is now three steps (upcoming events, concerns, social accounts) with a
-- precise concern catalog. Migrate stored profiles to the new contract:
--   * `drinking_drugs` expands to `underage_drinking` + `drug_use`;
--   * `other` (and any id outside the current catalog) has no operational meaning and is removed;
--   * results are de-duplicated, keeping the user's original selection order;
--   * a profile left with no concerns is reopened at the concerns step (2) with
--     `completed_at` cleared; consent fields are untouched;
--   * the retired fourth step is capped to 3;
--   * every changed row gets a revision bump so stale clients conflict instead of
--     overwriting migrated answers.
-- `coming_up` ids are unchanged (new ids are only additions), so it is not touched.

ALTER TABLE onboarding_profiles
  DROP CONSTRAINT IF EXISTS onboarding_profiles_current_step_check;

WITH expanded AS (
  SELECT p.tenant_id,
         p.id,
         e.value,
         (c.ord * 10 + e.sub) AS sort_key
  FROM onboarding_profiles p
  CROSS JOIN LATERAL unnest(p.concerns) WITH ORDINALITY AS c(value, ord)
  CROSS JOIN LATERAL unnest(
    CASE c.value
      WHEN 'drinking_drugs' THEN ARRAY['underage_drinking', 'drug_use']::text[]
      ELSE ARRAY[c.value]::text[]
    END
  ) WITH ORDINALITY AS e(value, sub)
  WHERE e.value IN (
    'inappropriate_language', 'underage_drinking', 'drug_use', 'hate',
    'political_takes', 'controversial_topics', 'negativity', 'public_image'
  )
),
deduped AS (
  SELECT tenant_id, id, value, min(sort_key) AS sort_key
  FROM expanded
  GROUP BY tenant_id, id, value
),
migrated AS (
  SELECT p.tenant_id,
         p.id,
         COALESCE(
           array_agg(d.value ORDER BY d.sort_key) FILTER (WHERE d.value IS NOT NULL),
           '{}'::text[]
         ) AS concerns
  FROM onboarding_profiles p
  LEFT JOIN deduped d ON d.tenant_id = p.tenant_id AND d.id = p.id
  GROUP BY p.tenant_id, p.id
)
UPDATE onboarding_profiles AS p
SET concerns = m.concerns,
    current_step = CASE
      WHEN cardinality(p.concerns) > 0 AND cardinality(m.concerns) = 0
        THEN LEAST(p.current_step, 2)
      ELSE LEAST(p.current_step, 3)
    END,
    completed_at = CASE
      WHEN cardinality(p.concerns) > 0 AND cardinality(m.concerns) = 0 THEN NULL
      ELSE p.completed_at
    END,
    revision = p.revision + 1,
    updated_at = now()
FROM migrated m
WHERE m.tenant_id = p.tenant_id
  AND m.id = p.id
  AND (p.concerns IS DISTINCT FROM m.concerns OR p.current_step > 3);

ALTER TABLE onboarding_profiles
  ADD CONSTRAINT onboarding_profiles_current_step_check
  CHECK (current_step BETWEEN 1 AND 3);
