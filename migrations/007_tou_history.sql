ALTER TABLE tou_rate_schedule ADD COLUMN utility_eia_id INTEGER;
ALTER TABLE tou_rate_schedule ADD COLUMN source_id TEXT;
ALTER TABLE tou_rate_schedule ADD COLUMN effective_start INTEGER;
ALTER TABLE tou_rate_schedule ADD COLUMN effective_end INTEGER;

-- Only exact upstream identities and integer timestamps can prove historical coverage.
UPDATE tou_rate_schedule SET
 utility_eia_id = json_extract(rate_json, '$.eiaid'),
 source_id = json_extract(rate_json, '$.label'),
 effective_start = json_extract(rate_json, '$.startdate'),
 effective_end = json_extract(rate_json, '$.enddate')
WHERE json_valid(rate_json)
 AND json_type(rate_json, '$.eiaid') = 'integer'
 AND json_type(rate_json, '$.label') = 'text'
 AND length(json_extract(rate_json, '$.label')) > 0
 AND json_type(rate_json, '$.startdate') = 'integer'
 AND (json_type(rate_json, '$.enddate') IS NULL OR json_type(rate_json, '$.enddate') = 'null'
      OR (json_type(rate_json, '$.enddate') = 'integer'
          AND json_extract(rate_json, '$.enddate') > json_extract(rate_json, '$.startdate')));

CREATE INDEX idx_tou_history ON tou_rate_schedule(utility_eia_id, rate_label, effective_start);

CREATE TABLE true_up_schedule (
 estimate_id INTEGER NOT NULL REFERENCES true_up_estimate(id),
 schedule_id INTEGER NOT NULL REFERENCES tou_rate_schedule(id),
 PRIMARY KEY (estimate_id, schedule_id)
);
INSERT INTO true_up_schedule SELECT id, tou_schedule_id FROM true_up_estimate;
