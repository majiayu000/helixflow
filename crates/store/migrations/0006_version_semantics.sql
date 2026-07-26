-- GH130 T6: semantic layer (capability/model/binding selections per node)
-- for versions created through the IntentPlan compiler path. NULL for
-- legacy v1 versions, which resolve through configured policy defaults.
ALTER TABLE versions ADD COLUMN semantics_json TEXT;
