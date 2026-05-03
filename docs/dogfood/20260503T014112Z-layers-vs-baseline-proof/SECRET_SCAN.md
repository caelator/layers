# Secret scan

Scope: `docs/dogfood/20260503T014112Z-layers-vs-baseline-proof`.

Checks run:

- Regex scan for common live secret forms: `sk-*`, GitHub tokens, Slack tokens, AWS access keys, and assignment-style `api_key`, `password`, `secret`, or `token` values of credential-like length.
- Direct search for known local credential names and provider key markers.

Result: no live credentials found.

Notes:

- Two regex hits were reviewed as false positives: source-code/preflight text discussing token accounting in `code-bugfix-provider-budget-overflow.preflight.json`. They are not credential values.
- No API keys, GitHub tokens, passwords, connection strings, or local keychain credential names were found in this artifact set.
