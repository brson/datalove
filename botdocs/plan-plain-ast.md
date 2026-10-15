# A Plain-Data AST

A plan for taking salsa out of the AST: expressions and functions become
ordinary data held by a memoized parse, the incremental boundary moves to an
explicit summary of each module's items, and later phases name functions and
expressions by stable keys rather than by salsa ids. Written October 2026,
after the parser stopped copying tokens ([plan-parser.md](plan-parser.md)).
Phases 0 to 2 and most of 3 are done; see [Progress](#user-content-progress).

## Contents

- [The problem](#user-content-the-problem)
- [What others do](#user-content-what-others-do)
- [Why not stop memoizing the parse](#user-content-why-not-stop-memoizing-the-parse)
- [The design](#user-content-the-design)
- [What stops naming the AST](#user-content-what-stops-naming-the-ast)
- [The firewall](#user-content-the-firewall)
- [Phases](#user-content-phases)
- [Progress](#user-content-progress)
- [Verifying each phase](#user-content-verifying-each-phase)
- [What it should buy](#user-content-what-it-should-buy)
- [Open questions](#user-content-open-questions)

## The problem

Every datafun expression is a salsa tracked struct (`ExprFun`), and so is
every call (`ExprFunctionCall`) and every function (`StmtFun`). Creating one
hashes its identity fields, finds it a disambiguator and writes it into a
salsa table page; reading one back is a table lookup. A cold compile of
`sys/std` creates 6,398 `ExprFun`s (972KB), 1,137 `StmtFun`s (246KB) and 949
`ExprFunctionCall`s (114KB), and `create_expr` alone is 3.3% of the run
(`query_census`, `perf`; 178.7M user instructions in all). Every later phase
pays again, a lookup per `.expr(db)`.

None of it buys anything at the expression grain. Every query that reads an
expression is keyed on the module, so an edit to one expression re-runs the
same queries whichever expression it was, and the tables that do want
per-expression identity already key on `ExprKey` -- module, function, index --
a plain value, not a salsa id. Tracking was taken off `ExprFun`'s fields for
exactly this reason ([salsa-patterns.md](salsa-patterns.md)); the struct
itself stayed, and the struct is what costs.

So the answer: the AST becomes plain data, still produced by one memoized
query per module, with functions and expressions named downstream by stable
keys, and a summary of each module's items as the firewall that the
tracked-struct identities provide today.

## What others do

- **salsa's tutorial** keeps statements and expressions plain: "We won't use
  any special 'Salsa structs' for expressions and statements", reusing work
  "at the granularity of functions" and advising "some kind of 'reasonably
  coarse' boundary" ([tutorial](https://salsa-rs.github.io/salsa/tutorial/ir.html)).
  Its API docs place tracked structs at "a derived entity owned by one query".
  The tuning guide's example of `lru` is a parse query.
- **rust-analyzer** memoizes `parse` with `lru = 128` and returns a plain
  syntax tree. `ItemTree` sits after it as "an 'invalidation barrier' ... when
  typing inside an item body, the `ItemTree` of the modified file is typically
  unaffected", `AstIdMap` gives items stable ids, and expressions are arena
  indices (`ExprId`) in a per-body store.
- **ty** memoizes its parse with `no_eq, lru = 200` over an ordinary Rust AST
  in an `Arc`. It opts out of comparison because its AST carries offsets:
  "comparing two ASTs is a non-trivial operation and every offset change is
  directly reflected in the changed AST offsets."
- **rustc** does not memoize parsing at all: the AST is fed into the query
  system unhashed, expansion and resolution re-run on every build, and reuse
  starts at the per-item HIR (`hir_owner`), fingerprinted and keyed by a stable
  `DefPathHash`.

None of them makes expressions salsa structs. All but rustc memoize the
parse; rustc's front end is not incremental at all, which its guide calls a
transition rather than a design.

## Why not stop memoizing the parse

Six things read a module's AST: name resolution, typechecking, ownership
analysis, lowering, the span table, and import discovery. Unmemoized, each
would parse for itself, and a cold compile would parse every module about six
times. An unchanged recompile, which runs no query at all today, would parse
the whole program, which the REPL and the reactive session pay on every
keystroke. rustc can afford that because its front end runs once per build;
ours is asked again on every revision.

What costs is not the memo, one `Arc` per module, but the salsa structs inside
it. Memoizing plain data costs what parsing it does, so the parse stays a
query.

## The design

**One query per module produces the AST, as plain data.** `parse_module_full`
keeps its shape -- statements, span tables, qualified calls -- but the
statements are ordinary values:

- A function is a `FunDecl`: name, `local_index`, `FunSignature` (as now, with
  `inline`), and a `FunBody`.
- A `FunBody` owns its expressions in an arena, `Vec<ExprNode>`, indexed by
  `ExprId(u32)`, and its statements, which refer to expressions by `ExprId`.
  The index is the `local_index` that `ExprKey` already carries, so
  `ExprKey { module, fn_name, local_index }` names an arena slot exactly as it
  names a struct today. Calls are numbered apart and get their own arena.
- `ExprNode` is today's `ExprFunKind`, with every child `ExprFun` replaced by
  an `ExprId`. The shapes do not change otherwise.
- Script-level code gets a `FunBody` of its own, keyed with `fn_name: None`.

The AST holds no spans outside parse-error nodes (spans are already in side
tables keyed by `ExprKey`), so it compares equal across an edit that moves
only spans. That is why it keeps `Eq` rather than taking ty's `no_eq`:
backdating on it is what keeps a blank line at the top of a file from
re-running typechecking.

**A handle stands in for `ExprFun`.** Code that walks expressions holds an
`ExprRef<'db>`, a `&'db FunBody` and an `ExprId`, `Copy` and two words. Its
`kind()` is a borrow of the node, with no salsa lookup and no `db` argument,
and its children come back as handles through the same body. `ExprKey::of`
becomes a field read. Functions likewise: `FunRef<'db>` is a module, a
`&'db FunDecl`, and the decl's index.

**The parse memo has no `lru` at first.** `sys/std` is 26 modules, and the
memos are smaller than the struct tables they replace. Salsa's LRU sits behind
one lock and cost ty 25% multi-threaded ([salsa#1032](https://github.com/salsa-rs/salsa/issues/1032)),
so it waits until memory asks for it.

## What stops naming the AST

Salsa owns a tracked struct through the query run that created it; a memo
holding its id is only good while that run's memo lives. So everything that
stores an AST id has to store something else before the AST can stop being
salsa's:

| Holder | Holds today | Becomes |
|---|---|---|
| `ResolvedCallTarget::func` | callee `StmtFun` | `FunKey { module: Option<ModuleId>, name }` |
| `ResolvedImport::func_ast`, `ModuleImportResolution::imports` | imported `StmtFun` | `FunKey` |
| `QualifiedFunction::ast` | callee `StmtFun` | `FunKey` |
| exports' `function_asts`, `function_stubs`, `fn_asts` | `StmtFun` | `FunKey`, or the module's own `FunRef` where it is the module's own |
| `analyze_script_expr_tracked` | an `ExprFun` argument | the unit and an `ExprId` |
| `typecheck_module`, `analyze_module`, `lower_module`, `TypecheckResult::root_parsed` | `ParsedStatements` as a key or field | the `Module` or unit, reading the AST inside |
| `ParsedModuleGraph::statements_only` | every module's statements, one tracked field | per-module `Module` handles; see [The firewall](#user-content-the-firewall) |

What is read through another module's `StmtFun` today is small: lowering takes
its name, ownership its parameters' `const` flags (which `TypeFunction`
already has), typechecking its `local_index` (to point a diagnostic at its
definition), its type parameters and bounds, and its `inline` flag. All of it
is signature, and all of it belongs in the summary below. Nothing reads
another module's body.

Taking the AST as a query argument also has to stop: a plain `ParsedStatements`
in a key is hashed whole on every lookup. The queries take the `Module` and
read `parse_module_ast(module)` inside, which is also the dependency they mean.

## The firewall

Today the firewall is identity. `ParsedStatements` holds `StmtFun` ids, a
body is a separately tracked field, and so `ret 1` to `ret 99` leaves the
statements equal and name resolution unrun (`parse_firewall_tests`, the third
row). With plain data the statements include the bodies, compare unequal, and
the firewall has to be written down: **`module_items(module)`**, a summary
that holds what other modules and the resolver see, and nothing a body edit
moves.

- requires and imports, with their `local_index`es;
- consts' names and kinds (their values are evaluated elsewhere);
- type aliases;
- function signatures: name, `local_index`, `FunSignature` (type parameters,
  bounds, parameters, return type, `inline`), and native functions';
- the qualified calls the module writes.

It is derived from `parse_module_ast`, compares by value, and backdates on any
edit inside a body. Name resolution (`resolve_module_names`), import
resolution and every cross-module signature read go through it, so the four
rows of `parse_firewall_tests` hold as they are:

| edit | full parse | AST | `module_items` | name resolution | typecheck |
|---|---|---|---|---|---|
| blank line at end | runs | backdates | -- | not run | not run |
| blank line at start | runs | equal | -- | not run | not run |
| `ret 1` to `ret 99` | runs | changes | equal | not run | runs |
| rename a function | runs | changes | changes | runs | runs |

The last column is what an edit already costs: the edited module's typecheck,
ownership and lowering re-run because they read its bodies. Its importers do
not, because they read its `module_items` and its resolution, which are equal.

`ParsedModuleGraph::statements_only` cannot stay as a field of every module's
statements: plain, it would change on any body edit anywhere and re-run every
graph-level reader. It becomes the module handles, and the graph-level passes
that walk it (lowering's strata, `compute_func_id_map`, the const kinds) read
`module_items` or `parse_module_ast` per module, inside per-module queries
where they do not already.

## Phases

Each phase leaves the compiler working, the suites passing, and the parse
snapshots unchanged.

0. **Make the harness see everything.** `ast_serde` omits call-site argument
   modes, type bounds and `inline`, so the parse snapshots cannot catch a
   change to them. Add them, bless the parser fixtures once, and rewrite the
   snapshot baseline. Pin the census (565 cold, 19 per edit) and the memory
   test's struct counts in the phase notes, to compare against.
1. **Expressions into arenas.** `ExprFun` and `ExprFunctionCall` stop being
   tracked structs: `FunBody` with its arenas, `ExprRef` handles, `ExprId`
   children. `StmtFun` stays a tracked struct, its `body` field now a
   `FunBody`, so the firewall and every incremental behavior are exactly as
   they are. This is most of the mechanical work -- about 74 uses in
   typechecking, 31 in lowering, 11 in ownership, the parser's `create_expr`
   and the script path -- and most of the win, since it removes the 6,398
   structs and the `.expr(db)` lookups.
2. **The summary, and keys for functions.** Add `module_items`; point name
   resolution, import resolution and the cross-module signature reads at it;
   replace every stored `StmtFun` in the table above with `FunKey`. `StmtFun`
   is still a tracked struct here, so this phase can be checked against the
   old firewall before the old firewall goes: the census and the incremental
   tests must not move.
3. **Functions as plain data.** `StmtFun` becomes `FunDecl`; `ParsedStatements`
   compares by value; the queries that took it as a key take the `Module`;
   `statements_only` becomes module handles. Here the firewall changes hands,
   from identity to `module_items`, and the incremental tests are the proof.
4. **What it opens up** (each its own change, measured): dense per-function
   tables in typechecking and ownership, indexed by `ExprId` where they are
   `BTreeMap<ExprKey, _>` today; `ResolvedCallTarget` and `TypeFunction` as
   plain values where nothing needs their identity; `lru` on the parse if
   memory wants it.

## Progress

**Phase 0** added the missing fields to `ast_serde` and re-blessed 16 parser
fixtures. The numbers it pinned, on the `sys/std` compile:

| | phase 0 | phase 1 |
|---|---:|---:|
| queries, cold / per edit | 566 / 19 | 566 / 19 |
| `ExprFun` structs | 6,227 (946KB) | none |
| `ExprFunctionCall` structs | 920 (110KB) | none |
| `StmtFun` structs | 1,130 (244KB) | 1,130 (289KB) |
| instructions | 178.1M | 158.7M |
| page faults | 3,794 | 3,687 |

**Phase 1** is done, and cost 11% of the run where the estimate was about 4%:
the lookups behind `.expr(db)` were worth more than `create_expr`. The parse
snapshots are identical, the census did not move, and no incremental test
changed. It differs from the design above in four ways:

- **The handle kept the name `ExprFun`.** It is `{ arena, id }` as designed,
  and the name kept about 200 signatures as they were. `kind()` borrows the
  node, `at(id)` makes a child's handle, and `key()` is `ExprKey::of`.
- **Calls are not in an arena of their own.** `ExprFunctionCall` is a plain
  node inside `ExprFunKind::FunctionCall`, carrying its call index, and
  `ExprKey::of_call(expr, call)` keys it.
- **The top level is a salsa struct, `TopLevel`, for now.** An `ExprFun`
  borrows its arena for `'db`, and a function's body gets that from its
  tracked field. The statements outside any function had no such owner while
  `ParsedStatements` is passed around by value, so they are put behind a
  tracked struct whose body is its identity. It therefore compares as the
  statements did. `ParsedExpr` uses the same struct for a script expression.
  This goes in phase 3, when the queries read `parse_module_ast(module)` by
  reference.
- **Walkers keep the current arena in their context**, as `arena` on
  `TypeContext`, `AnalysisCtx` and `LowerCtx`. It is set where a body is
  entered and restored after. Statements hold `ExprId`s, and `ctx.expr(id)`
  makes a handle from one.

**Phase 2's first half** replaced every stored `StmtFun` with a `FunHead`:
the declaring module, the name, the `local_index` and the `FunSignature`, by
value. That is where the design above said `FunKey`. A key needs a query to
look the signature up by, and the readers -- lowering, ownership, the
typechecker's arity and bound checks -- want only the signature, so carrying
it is simpler than looking it up. A script's own function and a rider's native
also have no module to look one up in. `FunHead` compares equal across a body
edit, as the `StmtFun` id did, and changes with the signature, as the
`TypeFunction` beside it already did, so nothing re-runs differently; the
incremental tests did not change. `FunKey` is the `Copy` part (module, name,
`local_index`), and the inline-cycle check keys on it. Rider natives stopped
being minted as `StmtFun`s with empty bodies.

**Phase 2's second half** added `module_items(module)`, an `Items` of the
module's requires, imports, type aliases, `FunHead`s and natives, with its
qualified calls. Name resolution, exports, `module_function_asts` and
`script_module_names` read it. Import resolution reads `module_uses`, which is
the same less the declarations. Reading `module_items` there made a signature
edit re-resolve the edited module's own imports, which `edit_reach_tests`
caught. Consts are not in it yet: what reads them, the const kinds and the
graph-level passes, is phase 3's. The census gained the two queries cold
(566 to 616) and nothing per edit.

**Phase 3's first step** removed `ParsedModuleGraph::statements_only`.
`typecheck_module`, `analyze_module` and `lower_module` are keyed on the
`Module` and read `parse_module_ast` inside; `lower_module` reads only
`module_items` unless it lowers from scratch. The graph-level passes walk the
graph's modules, `compute_func_id_map` numbers functions from `module_items`,
`parse_module_graph` reads riders' requires from `module_uses` and no longer
parses anything, and the untracked const and specialization passes ask for
each module's parse. The script compiler's specialization had looked a module
up by its `IrModuleId` as an index into `statements_only`, which is wrong
whenever a rider is numbered ahead of the modules; it looks the id up now.
Three query-log tests in `module_graph.rs` counted parses through
`parse_module_graph` and now parse each module themselves.

**Phase 3's second step** made `StmtFun` plain data, held in its parse,
with its signature behind an `Arc` that every `FunHead` made of it shares.
The firewall changed hands here, from identity to `module_items`, and no
incremental test changed. A body edit now runs 22 queries where it ran 19:
`parse_module_ast` and `module_items` run and backdate, and the stratum of
`lower_module_functions` that does not hold the edited function runs again,
since a stratum now reads the whole parse rather than its functions' body
fields. `module_import_demands` reads `module_uses`, and `module_const_kinds`
and `lower_module_functions` read `parse_module_ast` rather than
`parse_module_full`, so a span-only edit spares them.

The `FunHead` step had cost 4% of the `sys/std` compile (158.7M to 164.8M
instructions), every copy of a head cloning its signature; sharing the
signature took it to 151.4M, with page faults back to phase 1's (3,720).
A 512-module edit in `recompile_profile` went from 3.1ms to 2.25ms and peak
memory from 224MB to 215MB. When `StmtFun` was untracked in place, without a
summary to read instead, the same measurement got 32% slower and 62MB larger
(salsa-patterns.md).

`TopLevel` stays a tracked struct for now. Its identity is a hash of the whole
top level, functions included, made once per parse, and making it is 0.24% of
the `sys/std` compile, so there is little to win by removing it. Removing it
means giving the script-side queries that take it
(`analyze_script_fragment_tracked`, `collect_const_graph`) the script unit as
their key instead, and the borrows of a `ParsedStatements` held by value
somewhere other than a memo then live only as long as that copy.

**Phase 4, so far.** `ResolvedCallTarget` became a plain value in the
call-target table: 151.39M to 150.63M instructions. Then `ExprTypes` and
`CallTargets` became `BodyTable`s, a dense vector per body indexed by
`local_index`, in place of `BTreeMap`s keyed by `ExprKey`; nothing iterated
them, so `get` and `insert` kept their signatures and no reader changed. With
the typechecker moving its tables into its result rather than cloning them,
and the intrinsic-target table nobody read gone, the `sys/std` compile went
to 145.56M (page faults 3,709 to 3,653). On `recompile_profile` at 512
modules, an edit went from 24.6M instructions to 24.1M and the cold compile
from 4.62G to 4.34G, peak memory from 215MB to 202MB.

A `BodyTable` first found a body by a linear search of its bodies by name.
Each `ExprArena` now carries a `body` number -- 0 for the code outside any
function, a function's `local_index` plus one -- and the table is indexed by
it, taking an `ExprFun` rather than an `ExprKey`, so a lookup is two indexes.
Every reader already held the expression. That took the compile from 145.56M
to 140.63M instructions, more than the estimate, and a 512-module edit from
24.1M to 23.8M.

## Verifying each phase

- **Parse snapshots**, against a baseline written before the phase: identical,
  with phase 0's fuller AST serialization.
- **The three suites**, as always.
- **The incremental tests**, which assert what re-runs: `parse_firewall_tests`,
  `span_backdating_tests`, `incremental_memo_tests`, `import_memo_tests`,
  `edit_reach_tests`, `incremental_lowering_tests`, `compile_scaling_tests`,
  `durability_tests`, the `module_memo` fixtures and the script reactivity
  tests. They must pass unchanged through phases 1 and 2. In phase 3 any change
  to one is a change in what an edit re-runs and has to be argued for, not
  blessed.
- **`query_census`**: cold-compile and per-edit query counts, recorded per
  phase. Phase 1 should lower the cold count only by the queries that existed
  for expression structs, if any; phase 3 may change both.
- **`database_memory_tests`**: the struct counts it pins move in phase 1 and
  phase 3, on purpose; the bytes should fall.
- **Instructions and page faults** on the `sys/std` compile, per phase.

## What it should buy

Measured against today's 178.7M instructions:

- Phase 1: most of `create_expr`'s 3.3%, the lookups behind every `.expr(db)`
  in the later phases, and about 1.1MB of tracked-struct storage.
- Phase 3: the `StmtFun` table and identity hashing, and a simpler model --
  salsa tracks modules, items and query results; the AST is data.
- Phase 4 is where typechecking and ownership, about a fifth of the run and
  dominated by `BTreeMap` tables keyed by `ExprKey`, could get cheaper.

These are estimates to measure against, not predictions.

## Open questions

- **Should the arena be per function or per module?** Per function matches
  `ExprKey` and lets a body be handed around alone; per module is one
  allocation. Per function is the plan unless phase 1's numbers say the
  allocations matter.
- **Do parse-error nodes keep their spans?** They are the only nodes that do,
  so a file with a parse error stops backdating across span shifts. Moving
  them to the side tables would make the AST span-free; whether it is worth
  doing for files that do not compile is open.
- **Does `module_items` replace `TypeFunction`'s role?** `TypeFunction` is the
  resolved signature, made by name resolution from the written one; the
  summary holds the written one. Keeping both is the plan; merging them is
  phase 4 at the earliest.
