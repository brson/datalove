# GADTs Without Dependent Types

Design exploration for adding GADTs to Datalove while maintaining phase separation and building on existing const specialization infrastructure.

## Goals

- Enable GADTs (type refinement through pattern matching)
- No runtime type computation
- Clean phase boundaries
- Build on existing const specialization (union-branch strategy)
- Simple implementation

## Core Idea: Const-Indexed Types

Type parameters that must be const-known, using const values as type indices:

```datalove
enum TypeTag
    TInt
    TBool
end enum

enum Expr<const TAG: TypeTag>
    LitInt(i64)                 where TAG == TInt
    LitBool(bool)               where TAG == TBool
    Add(Expr<TInt>, Expr<TInt>) where TAG == TInt
    If(Expr<TBool>, Expr<TAG>, Expr<TAG>)
end enum
```

Where clauses constrain which const values are valid for each constructor. Pattern matching refines these constraints in each branch.

## Type-Level Const Functions

Const functions returning TypeTag enable computed return types:

```datalove
const fun ExprResult(tag: TypeTag): TypeTag
    ret tag
end fun
```

## Reify Operator

Maps TypeTag values to actual types (evaluated at compile time):

```datalove
type Reify<const TAG: TypeTag> =
    match TAG
        TInt => i64
        TBool => bool
    end match
```

## Complete Evaluator Example

```datalove
fun eval<const T: TypeTag>(e: Expr<T>): Reify<T>
    match e
        LitInt(n) =>
            // Compiler knows: T == TInt, so Reify<T> == i64
            ret n
        LitBool(b) =>
            // T == TBool, so Reify<T> == bool
            ret b
        Add(left, right) =>
            ret eval<TInt>(left) + eval<TInt>(right)
        If(cond, then_br, else_br) =>
            if eval<TBool>(cond)
                ret eval<T>(then_br)
            else
                ret eval<T>(else_br)
            end if
    end match
end fun
```

## Exhaustiveness

Constructors become unreachable when their where clause contradicts known constraints:

```datalove
fun negate(e: Expr<TBool>): Expr<TBool>
    match e
        LitBool(b) => ret LitBool(not b)
        If(c, t, f) => ret If(c, negate(t), negate(f))
        // LitInt, Add unreachable: TAG == TBool contradicts TAG == TInt
    end match
end fun
```

## Existential Hiding

For heterogeneous collections, pack the index existentially with a runtime witness:

```datalove
enum TypeWitness<const T: TypeTag>
    WitInt  where T == TInt
    WitBool where T == TBool
end enum

enum SomeExpr
    Pack<const T: TypeTag>(Expr<T>, TypeWitness<T>)
end enum
```

The witness enables runtime dispatch to recover type information.

## Length-Indexed Vectors

Const arithmetic in indices:

```datalove
enum Vec<const N: u64, T>
    Nil                    where N == 0
    Cons(T, Vec<N - 1, T>) where N > 0
end enum

fun concat<const A: u64, const B: u64, T>(
    v1: Vec<A, T>,
    v2: Vec<B, T>
): Vec<A + B, T>
    match v1
        Nil => ret v2
        Cons(x, rest) => ret Cons(x, concat<A - 1, B, T>(rest, v2))
    end match
end fun
```

## State Machine Types

```datalove
enum ConnState
    Closed
    Open
end enum

enum Connection<const S: ConnState>
    Conn(String, Reify<ConnData(S)>)
end enum

// Only callable when S == Open
fun send<const S: ConnState>(
    ref conn: Connection<S>,
    data: Bytes
): Result<(), Error> where S == Open
```

## Implementation Strategy

### Type Checker Changes

1. **Const evaluation during type checking** - invoke CTFE for type-level functions
2. **Constraint environment** - track const equalities from where clauses and pattern matches
3. **Satisfiability checking** - determine if constraints can all be true
4. **Reify reduction** - compute Type from const TypeTag
5. **Exhaustiveness with constraints** - recognize unreachable constructors

### Lowering

Extends existing union-branch strategy:

- Const-indexed types share runtime representation (unless layout differs)
- Where clauses become compile-time guards for branch reachability
- Monomorphic uses can elide type tags entirely

### Staged Implementation

1. **Phantom const indices** - `enum Foo<const N: u64>` where N doesn't affect representation
2. **Where clauses on constructors** - constraint refinement in pattern matches
3. **Type-level const functions** - `const fun` returning TypeTag, Reify operator
4. **Const arithmetic** - `Vec<N + M>` style expressions, SMT-lite solving

## Trade-offs vs Full Dependent Types

| Aspect | Const-Indexed | Full Dependent |
|--------|---------------|----------------|
| Types depend on | Const values only | Any runtime value |
| Phase separation | Clean | Blurred |
| Runtime overhead | None | Potentially significant |
| Implementation | Extends const specialization | Requires proof checking |

## Interaction with Linear Types

Composes cleanly - linearity checker tracks ownership as usual, GADT machinery is purely in type indices. No special interaction needed.
