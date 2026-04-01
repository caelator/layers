approved with changes

The module split is broadly sound and the migration order is mostly safe, but the plan should not be executed exactly as written. It documents a `synthesis -> memory` dependency that does not exist in the current code, and it places `existing_embeddings_requested()` in the graph module even though that function is part of refresh/analyze behavior, not query-time graph retrieval.

Fix those boundaries first, then proceed with the refactor.
