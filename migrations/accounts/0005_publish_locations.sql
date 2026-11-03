-- Per-account publish location override.
--
-- Historically the publish location (发布地 / fishAdCode / gps) was baked into
-- LocationContext::default() (株洲荷塘区, 430202). For sellers operating from
-- different cities this caused all upstream MTOP calls to leak the wrong gps,
-- making the marketplace surface their listings under the wrong adcode.
--
-- This table stores a per-uid override. When a row exists for a uid the
-- backend uses these values for every publish/groupon/pricing/orders flow;
-- when no row exists the legacy default still applies, so existing accounts
-- keep working until the user configures them via the accounts UI.

CREATE TABLE IF NOT EXISTS publish_locations (
    uid TEXT PRIMARY KEY,
    fish_ad_code TEXT NOT NULL,
    gps TEXT NOT NULL,
    latitude TEXT NOT NULL DEFAULT '',
    longitude TEXT NOT NULL DEFAULT '',
    province TEXT NOT NULL DEFAULT '',
    city TEXT NOT NULL DEFAULT '',
    district TEXT NOT NULL DEFAULT '',
    updated_at INTEGER NOT NULL DEFAULT 0
);
