# task-critical-review

Review the code in question carefully as a subject expert.
Look for:
correctness,
conciseness,
encapsulation,
readability,
documentation.

Look for opportunities to share code
where it will prevent errors in the future.

Do not allow defensive and dead code:
breaking contracts should trigger panics,
not enter fallback code nor compatibility code,
not hide recoverable errors.

Write concise but complete docs.

Refresh yourself by reading
botdocs/botspec.md and
botdocs/compiler-guide.md
