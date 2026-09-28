-- One row per unique contact; `contact` is normalized (lowercased email or E.164 phone).
CREATE TABLE waitlist (
  id INTEGER PRIMARY KEY,
  contact TEXT NOT NULL UNIQUE,
  kind TEXT NOT NULL CHECK (kind IN ('email', 'phone')),
  source TEXT CHECK (source IN ('hero', 'cta')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
