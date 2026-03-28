# Carry/Bring Loops: Removed Feature

Carry/bring was an SSA-style loop iteration state mechanism.
It was fully implemented across the entire compiler stack
(parser, AST, typechecker, drop analysis, IR block parameters,
interpreter, AOT/Cranelift),
then removed in commit `cbc756c2`
as experimental complexity that didn't prove necessary.

This topic keeps coming up in design discussions.
This document records what it was and why it was removed.


## Syntax

```datalove
// Carry only: iteration state.
loop carry (acc: u32 = @1, i = n)
    if i ≤ @1
        break
    end if
    continue(acc * i, i - @1)
end loop

// Bring only: exit values.
loop
    set x = x + @1
    if x > threshold
        break(x)
    end if
end loop bring (found: u32)
ret found

// Combined carry and bring.
loop carry (acc: u32 = @1, i = n)
    if i ≤ @1
        break(acc)
    end if
    continue(acc * i, i - @1)
end loop bring (result: u32)
ret result

// With while condition.
loop carry (i: u32 = @0) while i < limit
    set total = total + i
    continue(i + @1)
end loop
```


## Rules

- `carry` declared loop state bindings with initial values.
  Type annotations optional (inferred from init).
- `continue(values...)` passed new values to the next iteration.
  Plain `continue` was forbidden when carries existed.
- `bring` declared bindings assigned when the loop exited.
  Type annotations required.
- `break(values...)` provided bring values on exit.
  Plain `break` was forbidden when brings existed.
- Loops with carries required no fallthrough:
  every path had to explicitly `break`, `continue(...)`, or `ret`.
- `loop while` with brings required an `else break(values)` clause
  for the case where the condition was initially false.


## Implementation

Used SSA block parameters (not phi nodes).
Loop header block had params for carries;
exit block had params for brings.
`Goto`/`Branch` terminators passed args to target blocks.

Known issues at time of removal:
- Bring bindings required type hints in interpreter tests.
- Bigint carry values caused drop analysis `UseAfterMove` errors.
- AOT bigint loops timed out (pre-existing issue, not carry-specific).


## Why removed

Commit message: "The carry/bring feature was experimental
and added complexity without proving necessary."

Plain `var`/`set` handles loop iteration state:

```datalove
var acc: u32 = @1
var i = n
loop
    if i ≤ @1
        break
    end if
    let new_acc = acc * i
    set i = i - @1
    set acc = new_acc
end loop
ret acc
```

The var/set version is more verbose but uses no special syntax.
The carry/bring mechanism added significant complexity
to every compiler phase for a feature that `var`/`set` already covers.


## Archived documents

- Implementation plan: `git show e53b9664:botdocs/plan-carries.md`
- Semantics addendum: `git show c2b6e381:botdocs/plan-carries.md`
- Botspec diff showing full syntax docs: `git diff cbc756c2~1..cbc756c2 -- botdocs/botspec.md`
