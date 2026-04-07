1. **Section: 3. Recommended approach with rationale**
   - **Suggest:** Replace “lazy initialization” with an explicit startup preload step that parses `route-corrections.jsonl` once during app/bootstrap, then inject the resulting in-memory corrections into `Router`.
   - **Why:** The success criteria explicitly require the file to be read “at startup,” and lazy loading can hide parse failures until first route use, making startup behavior and test coverage ambiguous.

2. **Section: 4. V1 scope**
   - **Suggest:** Add a small internal `FeedbackContext` struct passed through execution/query boundaries that carries `task`, predicted route, and any fallback/actual route needed to construct `RouteFailure`, instead of assuming `stage.rs` or `query.rs` can reconstruct that data locally.
   - **Why:** The current plan depends on failure emitters having full routing context, but the risk section already hints they may not. Making that context explicit avoids partial or malformed failure records and keeps the wiring localized.

3. **Section: 4. V1 scope**
   - **Suggest:** Narrow “low-quality” soft-failure detection to one concrete, testable rule for V1, such as “empty result set” plus one existing numeric threshold if the codebase already exposes a score/confidence field; otherwise defer broader quality heuristics.
   - **Why:** “Confidence is critically low” is underspecified and likely to create flaky behavior or accidental over-logging. A stricter V1 rule keeps behavior deterministic and reduces regression risk.

4. **Section: 8. Control flow**
   - **Suggest:** Insert a deduplication guard for repeated hard failures within the same run/stage attempt, so a timeout or non-zero exit emits at most one failure record per routed attempt.
   - **Why:** Without this, retries, polling loops, or repeated status checks can inflate weights from one incident and distort future routing more than intended.

5. **Section: 9. Risks and open questions**
   - **Suggest:** Add a concrete compatibility rule for startup parsing: skip malformed JSONL lines with a warning/counter instead of failing startup, while preserving backward-compatible parsing for known `RouteFailure` variants.
   - **Why:** Existing logs may contain old or corrupted lines. Hard-failing on one bad line would violate the “successfully parses `route-corrections.jsonl` without errors” requirement in real environments.

6. **Section: 10. Validation plan**
   - **Suggest:** Add a serialization round-trip compatibility test using historical JSON fixtures for `RouteFailure`, including at least one hard and one soft entry from the current log format.
   - **Why:** Backward compatibility is a hard constraint, but the current validation plan only tests new writes and routing behavior, not whether older persisted records still deserialize correctly.

7. **Section: 10. Validation plan**
   - **Suggest:** Add an integration-style test that bootstraps the app/router with a temp `route-corrections.jsonl` path and verifies both phases together: startup load succeeds and a known penalized route changes the final routing decision or score ordering.
   - **Why:** The current tests split write-path and router behavior, but the main risk is the end-to-end loop not actually influencing live routing after startup load.
