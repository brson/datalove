## task-no-defensive-code

Our process tends to result in defensive
and fallback code sitting around that
either does nothing or actively obscures bugs.

Look for suspicious "fallback"
and "backwards compatibility" branches
that are impossible based on the invariants
of e.g. previous compiler passes,
remove them or convert to panics.

Look for arguments that take `Option`
where the `None` case is not used for any production
purpose.




## task-critical-organization

Review the code for logical and top-down organization.

Our code should read naturally top-down,
starting with entry points and key datatypes,
proceeding to implementations and utilities and tests.

Where a single module contains multiple logical
groupings of types or functionality,
consider extracting it to its own module.

We are very focused on establishing dependency
and modules DAGs, creating very firm
separation of concerns between functions modules and crates,
letting them interact through shared data types,
not shared behavior.

Firm boundaries make maintenance easier.

We use lots of crates arranged as peers
with diamond dependencies for shared types.

Re-exports




## task-critical-comment-review

Review comments and doc comments for
correctness,
completeness,
conciseness.

Write straightforward docs like I would,
or my hero Hemmingway would.
Not too listy.

Module crate docs should provide a sufficient architectural overview,
with entry points and examples if appropriate.

Non-doc-comments should be minimial and concise,
explaining non-obviousness and invariants,
and not restating what is clear from the code and its clear naming and style.




## task-critical-review

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
