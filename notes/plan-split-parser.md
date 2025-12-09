# Task: Give datafun its own datalit expression parser.

Today datafun attempts to reuse the datalit parser,
and possibly its typechecker,
for parsing some expressions and type hints.

This isn't tenable though.

The substructure of datafun expressions must
recursively contain expressions that aren't in datalit,
like binops and names.

So when the datafun expression parser defers
to the datalit expression parser,
the subsequent nested expressions can't include
binops etc.

To remediate this we need datafun to duplicate
the datalit expression parser, with branches
for datafun-specific expressions.

This will probably also mean duplicating the
datalit typechecker into the datafun expression type checker.

To ensure the datafun expression parser maintains
a strict subset of datalit we'll need a test suite that:
parses ast_gen generated expressions in both
and ensures they are equivalent and typecheck equivalently.

# Plan: todo