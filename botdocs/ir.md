# The Datafun IR

The reference for `datalove-datafun-ir`: the SSA intermediate representation
that lowering produces and that every backend consumes.

## Contents

- [1. What the IR Is](#user-content-1-what-the-ir-is)
- [2. Identifiers](#user-content-2-identifiers)
- [3. Types](#user-content-3-types)
- [4. Layout](#user-content-4-layout)
- [5. Code Units](#user-content-5-code-units)
- [6. Blocks and Operands](#user-content-6-blocks-and-operands)
- [7. Instructions](#user-content-7-instructions)
- [8. Terminators](#user-content-8-terminators)
- [9. Ownership and Tracking](#user-content-9-ownership-and-tracking)
- [10. Constants](#user-content-10-constants)
- [11. Generics and Descriptors](#user-content-11-generics-and-descriptors)
- [12. Registries](#user-content-12-registries)
- [13. Passes](#user-content-13-passes)
- [14. Consumers](#user-content-14-consumers)
- [15. Text and Serialized Forms](#user-content-15-text-and-serialized-forms)
- [16. Invariants](#user-content-16-invariants)
- [17. Tests](#user-content-17-tests)

## 1. What the IR Is

The IR is flat, typed and in SSA form. It sits between the typechecked AST and
the four things that run code -- the interpreter, the Cranelift AOT backend, the
C backend and the JIT -- and it is the last representation any of them share.
A phase that disagrees with another about the IR does not fail a build; it
writes bytes one way and reads them another.

Three decisions shape it.

**Immutable values, explicit slots.** An expression temporary or a `let`
binding is a `ValueId`, defined exactly once. A `var` binding is a `SlotId`,
which can be stored to and loaded from. The split means the SSA half needs no
phi nodes for ordinary code and maps onto registers, while the mutable half
maps onto stack storage.

**Block parameters instead of phis.** Where a value does have to differ by the
path taken -- a loop carry, a `break` with a value -- the target block declares
a parameter and the terminator passes an argument. Arguments are *moved* into
the parameter, which is what makes the scheme work for linear values.

**Ownership is written down.** Lowering consumes the ownership analysis's
`DropSchedule`, so every move, every clone and every drop is an instruction.
No backend infers when to destroy a value; it does what the IR says.

The IR is serializable and database-independent. Nothing in it is a salsa id,
deliberately: a salsa id is an index and a generation, meaningful only in the
revision that minted it, and the backends must not be able to see any of that.

The crate is `crates/datalove-datafun-ir`, with four modules -- `layout`,
`params`, `registry` and `display` -- and the type definitions in `lib.rs`.

## 2. Identifiers

| Type | Meaning |
|------|---------|
| `ValueId(u32)` | SSA value, defined once, immutable |
| `SlotId(u32)` | Mutable slot, for `var` bindings |
| `ParamId(u32)` | Function parameter, a reference to the caller's data |
| `BlockId(u32)` | Basic block |
| `CodeUnitId(u32)` | Code unit, numbered within its containing scope |
| `FuncId(u32)` | Module-local function, in the compiler's `FuncIdMap` |
| `IrModuleId(u32)` | Module index; *not* salsa's `ModuleId` |

`CodeUnitId` is the unified addressing scheme that replaced `FuncId` inside the
IR. `FuncId` survives above it, in the compiler's map from `(ModuleId, name)` to
`(IrModuleId, FuncId)`; where both appear they are numerically the same.

`IrModuleId` is a plain number so that a module reference can be serialized.
Salsa's `ModuleId` is a `#[salsa::input]`, so two `::new()` calls on the same
path give different ids; nothing in the IR can depend on that.

### Code Unit References

```rust
pub enum CodeRef {
    Local(CodeUnitId),                             // Same scope
    External { unit: u32, id: CodeUnitId },        // Previous script unit
    Module { module: IrModuleId, id: CodeUnitId }, // A compiled module
}
```

**`Local` is relative, and is not a name to remember a function by.** It is a
position in whichever unit's list is in scope, and every script unit numbers its
own functions from zero, so `Local(1)` means a different function in each of
them. Resolving one is fine, because the execution context holds one unit's list
at a time and only `External` swaps it. What is not fine is *remembering* a
function between calls by a `Local` alone: the JIT's compiled bodies did, and a hot function in one REPL line
was executed in place of a different function at the same id in the next.
Anything that caches keys on a resolved identity instead -- `FuncIdentity`,
which the interpreter and the JIT share.

## 3. Types

`IrType` is the IR's own type representation: self-contained, serializable, and
independent of the database. It is what frame layout and runtime type
descriptors are computed from.

| Variant | Notes |
|---------|-------|
| `Unit` | The empty tuple; zero-sized |
| `Bool` | |
| `U8` `U16` `U32` `U64` | |
| `I8` `I16` `I32` `I64` | |
| `Index` `Offset` | Width depends on the `index-64` feature |
| `Int` | Arbitrary-precision integer |
| `F32` `F64` | |
| `String` | |
| `Data` | The language's `data`: a value carrying its own descriptor |
| `Error` | Error value |
| `Tuple(Vec<IrType>)` | Fields in order |
| `Struct(Vec<(String, IrType)>)` | Fields sorted by name |
| `Enum(Vec<(String, Option<IrType>)>)` | Variants sorted by name |
| `Atom(String)` | Zero-sized named tag |
| `Term(String, Box<IrType>)` | Transparent over its payload |
| `List(Box<IrType>)` | |
| `Set(Box<IrType>)` | |
| `Map(Box<IrType>, Box<IrType>)` | |
| `Option(Box<IrType>)` | |
| `Result(Box<IrType>)` | Carries the ok type; the error type is fixed |
| `Tensor(Box<IrType>, u32)` | Element type and rank |
| `Table(Vec<(String, Box<IrType>)>)` | Columns in the order they were written |
| `Ref(Box<IrType>)` | A pointer to a value of the inner type |
| `Param(u32)` | A type parameter of the enclosing function, by position |

Two ordering rules matter and differ.

**Struct fields and enum variants are sorted by name.** A discriminant is an
index into the sorted variant list, so every layer that builds one has to sort
the same way or a conversion reads one variant's payload at another's offset.

**Table columns are kept in the order they were written.** Sorting them agreed
with nothing else: datalit compares two table types column by column, the
typechecker makes a literal's header match its type, and a literal's cells are
lowered left to right. A table whose columns were not already alphabetical had
every cell written at another column's offset.

`IrType::Ref` has no source-level counterpart. It is produced by the
projection instructions (`GetFieldRef`, `ListElementRef`, `MapValueRef`,
`TensorIndexRef`) and consumed where a projection is passed to a `ref`, `mut` or
`out` parameter. It is pointer-sized and always copy.

### Conversions into `IrType`

- `IrType::from_type_hint` converts a written type.
- `IrType::from_type_hint_generic` does the same inside a generic, keeping
  its type parameters as `Param` (Section 11).
- `IrType::from_datalit` converts a typechecker type outside any generic, and
  `from_datalit_generic` inside one, keeping its type parameters as `Param`.
  A type variable that is not a parameter in scope is a compiler bug, and
  panics.

### `TypeRef`

A lightweight tag without inner detail, used by one instruction -- `Pack` --
to say what shape is being built: the scalars, `Tuple(n)`, `AnonStruct(n)`,
`Option`, `Result`, `List`, `Set`, `Map`.

### `is_copy`

`IrType::is_copy` answers whether a value can be duplicated by a shallow
bitwise copy. The scalars, `Unit`, `Atom` and `Ref` are copy. The heap types
are not. A tuple, struct, enum, term or option is copy when everything it holds
is. A `Result` is never copy, because its error arm holds an `Error`.

## 4. Layout

`datalove_datafun_ir::layout` is the single authority on how an `IrType` is laid
out in memory. It is also only half of the story: `datalove_rtdt::layout`
computes the same layouts from runtime type descriptors, and the runtime reads
values back through those. Generated code writes at offsets from the first and
the runtime reads at offsets from the second, so a disagreement corrupts values
rather than failing a build.

```rust
pub struct TypeLayout { pub size: u32, pub align: u32 }
pub fn layout_of(ty: &IrType) -> TypeLayout;
```

The rules:

- `Unit` and `Atom` are zero-sized with alignment 1.
- The scalars have their natural size and alignment; `Index` and `Offset` take
  theirs from `rtdt::INDEX_SIZE` and `rtdt::INDEX_ALIGN`.
- `Int`, `String`, `Data`, `Error`, `List`, `Set`, `Map`, `Tensor` and `Table`
  have the size and alignment of the corresponding runtime struct.
- `Ref` is a pointer.
- A tuple or struct lays its fields out in order, each at its own alignment,
  with the whole rounded up to the widest field's alignment.
- A `Term` is transparent over its payload.
- An enum is a `u32` discriminant followed by the payload, each variant's
  payload at the first offset past the discriminant that satisfies *that
  variant's* alignment. Variants of differing alignment therefore have
  differing payload offsets.
- A `?T` is a one-byte tag followed by the payload at the payload's alignment.
- A `!T` is a one-byte tag followed by the larger of the ok payload and the
  error payload, at the greater of their alignments. Both arms share the
  offset.

Nothing should open-code this arithmetic. The payload offsets have named
functions in both authorities:

| | `ir::layout` | `rtdt::layout` |
|---|---|---|
| Enum variant | `enum_payload_offset(ty)` | `enum_payload_offset(align)` |
| `?T` | `option_payload_offset(ty)` | `option_payload_offset(align)` |
| `!T` | `result_payload_offset(ty)` | `result_payload_offset(align)` |

Also exported: `align_up`, `aggregate_layout`, `aggregate_field_offsets`,
`enum_layout`, `enum_variant_offsets`, `option_layout`, `result_layout`.

**A container's layout does not depend on what it holds.** A `[u32]` and a
`[string]` occupy the same slot, and so do the set, map and table cases. A
tuple is the opposite case: `(u32, u32)` and `(u8, u32)` are different shapes.
Both facts are pinned by unit tests in `layout.rs`.

**A dynamic type is laid out as a pointer**, one word, whatever it is:
`layout_of` checks `is_dynamic` before its match. The offset helpers above are
never asked about a dynamic type; its layout is its descriptor's (Section 11).

## 5. Code Units

One type represents functions, script units and native functions. The `context`
field decides the execution semantics.

```rust
pub struct IrCodeUnit {
    pub id: CodeUnitId,
    pub name: String,

    // Body.
    pub blocks: Vec<IrBlock>,
    pub value_count: u32,
    pub slot_count: u32,
    pub value_types: Vec<IrType>,   // Indexed by ValueId
    pub slot_types: Vec<IrType>,    // Indexed by SlotId
    pub tracked_slots: Vec<SlotId>,
    pub const_values: Vec<(String, ValueId)>,
    pub symbols: SymbolTable,

    // Context.
    pub context: CodeUnitContext,

    // Units defined inside this one.
    pub nested_units: Vec<IrCodeUnit>,
}

pub enum CodeUnitContext {
    Function(FunctionContext),
    Script(ScriptContext),
    Native(NativeContext),
}
```

`entry_block()` is `blocks[0]`. `is_function`, `is_script`,
`function_context`, `script_context`, `native_context` and their `_mut`
counterparts read the context; `return_type` and `param_count` answer for a
function and give nothing, or zero, for anything else.

### FunctionContext

```rust
pub struct FunctionContext {
    pub params: Vec<ParamId>,
    pub param_modes: Vec<ParamMode>,
    pub param_types: Vec<IrType>,
    pub return_type: IrType,
    pub tracked_params: Vec<ParamId>,
    pub type_param_count: u32,
    pub descriptor_table: Vec<IrType>,
    pub inline: bool,
}
```

`tracked_params` are the `out` parameters, which need a runtime tracking byte
because they start uninitialized. `type_param_count` and `descriptor_table`
belong to generics and are described in Section 11. `inline` says every call
to the function is replaced by its body (Section 13).

### ScriptContext

```rust
pub struct ScriptContext {
    pub unit_end_values: Vec<ValueId>,
    pub unit_end_slots: Vec<SlotId>,
    pub result: Option<ValueId>,
    pub result_name: Option<String>,
    pub exports: Vec<(String, ExportBinding)>,
}
```

A script unit is one REPL line or one chunk of a script. `exports` names what
later units can reach:

```rust
pub enum ExportBinding {
    Value(ValueId),
    Slot(SlotId),
    Function(CodeUnitId),
}
```

`unit_end_values` and `unit_end_slots` are what the unit would destroy at its
end. The interpreter does not, because a REPL binding has to outlive the line
that made it; the AOT backends do.

`result_name` is the case where an expression unit is a bare name. The prompt
is asking to see a binding an earlier unit owns, so there is nothing to compute
and nothing to copy: the name is recorded, `result` is empty, and the executor
reads that binding where it lives.

### NativeContext

```rust
pub struct NativeContext {
    pub param_modes: Vec<ParamMode>,
    pub param_types: Vec<IrType>,
    pub return_type: IrType,
    symbol: String,                   // e.g. "dlr_std__list_push", read by symbol()
    pub undetermined_type_params: Vec<u32>,
}
```

A native function has a signature and a linker symbol and no blocks. It is
built with `NativeContext::new(rider, function, ...)`, which spells the symbol
by `native_symbol`, so nothing outside the IR crate knows the format.
`undetermined_type_params` are the type parameters no argument determines, by
position; a call hands over a descriptor for each, after the result. See
[the native ABI](native-abi.md).

### Parameter Modes

```rust
pub enum ParamMode { In, Out, Ref, Mut }
```

Call sites repeat the mode, and typechecking rejects any disagreement with the
declared one (F057), so later phases can read the mode off the call site alone.
Ownership analysis does exactly that, which is why it needs no resolved call
target to know how an argument is passed.

Reading a parameter is `Operand::Param(p)` rather than an instruction. Writing
one has four forms, split by whether the destination is precise (`Mut`, which
the caller always hands over initialized) or tracked (`Out`, which starts
empty): `ParamStore`, `ParamStoreTracked`, `ParamSetField`,
`ParamSetFieldTracked`. `RefStore` and `RefSetField` generalize these to any
reference-like operand.

For an `out` parameter the **caller** destroys the existing value before the
call, as an instruction of its own: `drop.ref` through a reference to an
element or field, `drop.tracked` for a tracked binding or the caller's own
`out` parameter, which may hold nothing, and `drop` otherwise. A call does
nothing to its `out` destinations itself, so every engine, and anything that
moves or removes a call, sees the whole rule in the IR.

No store destroys what its destination held. Lowering emits the destroy as its
own instruction in front of the store, and only where the stored type owns
something (`IrType::is_copy` is false): `drop` for a slot or a `mut`
parameter, `drop.tracked` for a tracked slot or an `out` parameter, `drop.ref`
through a reference, and `drop.field` for a field of any of them. A store of a
copy type, an `f64` or a struct of them, is then a bare write in every engine,
and no backend decides for itself whether a store needs a destroy.

### IrModule and SymbolTable

```rust
pub struct IrModule {
    pub functions: Vec<IrCodeUnit>,
    pub symbols: SymbolTable,
}
```

`SymbolTable` holds `FuncDef { id, name, param_count }` indexed by `FuncId`,
plus a name lookup that exists only during lowering and is not serialized. Its
`PartialEq` and `Hash` ignore that transient map, so two tables that define the
same functions compare equal however they were built -- which is what salsa
needs of anything it memoizes.

## 6. Blocks and Operands

```rust
pub struct IrBlock {
    pub id: BlockId,
    pub params: Vec<ValueId>,
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
}
```

Block parameters take the place of phi nodes. Each has a fixed frame location;
a `Goto` or `Branch` moves its arguments into those locations, transferring
ownership. The interpreter copies the data and marks the source dropped; the
Cranelift backend uses a real block parameter for a scalar and a memcpy into the
frame slot for an aggregate.

```rust
pub enum Operand {
    Value(ValueId),
    ValueRef(ValueId),                     // Auto-dereferenced on use
    Slot(SlotId),
    Param(ParamId),
    ExternalValue { unit: u32, value: ValueId },
    ExternalSlot { unit: u32, slot: SlotId },
}

pub enum SlotDest {
    Local(SlotId),
    External { unit: u32, slot: SlotId },
}
```

`ValueRef` is how a projection is used. The value holds a pointer -- it came
from `GetFieldRef` or one of the element-reference instructions -- and reading
the operand dereferences it.

The `External` variants are the REPL: a later unit reading or writing a binding
an earlier one owns.

## 7. Instructions

Instructions are flat, with two or three operands at most, and no nesting. The
tables below give the variant, the printed mnemonic, and what it does to
ownership. "Borrows" means the operand survives; "consumes" means it does not.

### Constants and Movement

| Instruction | Printed | Ownership |
|---|---|---|
| `Const { dest, value }` | `v0 = const 42int` | Produces `dest` |
| `StaticRef { dest, value }` | `v0 = staticref [1int, 2int]` | Produces `dest`, a borrow of the one copy the backend built; nothing drops it |
| `Copy { dest, src }` | `v0 = copy v1` | Borrows `src`; copy types only |
| `Move { dest, src }` | `v0 = move v1` | Consumes `src`, shallow |

### Arithmetic

| Instruction | Printed | Ownership |
|---|---|---|
| `BinOp { dest, op, lhs, rhs }` | `v0 = add v1, v2` | Borrows both |
| `UnaryOp { dest, op, operand }` | `v0 = neg v1` | Borrows |
| `BinOpChecked { dest, overflow, op, lhs, rhs }` | `v0, v1 = add.checked v2, v3` | Borrows both |
| `UnaryOpChecked { dest, overflow, op, operand }` | `v0, v1 = neg.checked v2` | Borrows |
| `OpAssign { place, op, rhs }` | `add.assign s0, v1` | Borrows `rhs`; updates the place |
| `OpAssignChecked { overflow, place, op, rhs }` | `v0 = add.assign.checked s0, v1` | Borrows `rhs`; updates the place unless it fails |
| `Widen { dest, src }` | `v0 = widen v1` | Borrows; produces an `Int` |
| `WidenFixed { dest, src }` | `v0 = widen_fixed v1` | Borrows; both copy |
| `Clone { dest, src }` | `v0 = clone v1` | Borrows; deep copy, the `@` operator |

`BinOp` covers `Add Sub Mul Div Mod Eq Ne Lt Le Gt Ge And Or BitAnd BitOr
BitXor Shl Shr LogicAnd LogicOr LogicXor`; `UnaryOp` covers `Neg Not BitNot
LogicNot`. The checked forms produce the result and a separate overflow flag,
and the branch on that flag is what the source's `+!` and `+?` become.

`OpAssign` and `OpAssignChecked` are what `set p op= v` lowers to: the place,
a slot, parameter or reference as for `RefStore`, is updated where it is. An
`int` place is updated by the runtime, which reuses its buffer when it has room
(`dtlv_rti_int_add_assign` and its siblings), and `rhs` may be the place itself.
A checked update that fails leaves the place as it was. Their types are `f32`,
`f64`, `int` and the fixed-width integers; a dynamic place's arithmetic goes
through a `BinOp` and a store, since the runtime picks it by descriptor.

`WidenFixed` zero-extends an unsigned source and sign-extends a signed one.

### Calls

| Instruction | Printed |
|---|---|
| `Call { dest, func, args, bindings }` | `v0 = call m1.u2(v1, v2)` |
| `ComptimeCall { dest, func, args, comptime_param_indices, bindings }` | `v0 = comptime_call u1(v1) [disc=2, comptime_params=[0]]` |

Arguments are consumed or borrowed according to the callee's parameter modes.
`bindings` is what the site bound each of the callee's type parameters to
(Section 11), and is empty for a call to a non-generic callee.

`ComptimeCall` is emitted where the callee has const parameters. Left alone it
behaves exactly like `Call`. The specialization pass points it at the copy
built for its const arguments' values, as a `Call` without the const
arguments.

### Aggregates

| Instruction | Printed | Ownership |
|---|---|---|
| `Pack { dest, ty, fields }` | `v0 = pack Tuple2 {v1, v2}` | Consumes all fields |
| `Unpack { dests, src }` | `(v0, v1) = unpack v2` | Consumes `src` |
| `GetField { dest, src, field_index }` | `v0 = getfield v1.0` | Consumes `src`; the field moves out |
| `GetFieldRef { dest, src, field_index }` | `v0 = getfieldref v1.0` | Borrows `src`; produces a `Ref` |

### Option, Result and Enum

| Instruction | Printed | Ownership |
|---|---|---|
| `WrapSome { dest, inner }` | `v0 = some v1` | Consumes `inner` |
| `WrapNone { dest }` | `v0 = none` | Produces `dest` |
| `WrapOk { dest, inner }` | `v0 = ok v1` | Consumes `inner` |
| `WrapErr { dest, inner }` | `v0 = err v1` | Consumes `inner` |
| `EnumVariant { dest, variant_index, payload }` | `v0 = enum_variant 1 v1` | Consumes the payload |
| `EnumDiscriminant { dest, src }` | `v0 = enum_discriminant v1` | Borrows `src` |
| `EnumPayload { dest, src, variant_index }` | `v0 = enum_payload v1 2` | Consumes `src` |
| `UnwrapOption { dest, is_some, src }` | `v0, v1 = unwrap_option v2` | Consumes `src` |
| `UnwrapResult { ok_dest, err_dest, is_ok, src }` | `v0, v1, v2 = unwrap_result v3` | Consumes `src` |

`variant_index` is an index into the *sorted* variants of the enum type.

`match` lowering emits `EnumDiscriminant` to read the tag and then a chain of
comparisons branching to the arm blocks. An atom arm drops the input; a term arm
takes its binding with `EnumPayload`.

The two unwrap instructions produce their payload together with a flag. The
payload is valid only on the arm the flag selects, and the backend must not read
it otherwise.

### Boxing

| Instruction | Printed | Ownership |
|---|---|---|
| `ErrorFrom { dest, inner }` | `v0 = error_from v1` | Consumes `inner` |
| `DataFrom { dest, inner }` | `v0 = data_from v1` | Consumes `inner` |

### Collection Construction

| Instruction | Printed | Ownership |
|---|---|---|
| `ListNew { dest, elements }` | `v0 = list [v1, v2]` | Consumes all elements |
| `SetNew { dest, elements }` | `v0 = #{v1, v2}` | Consumes all elements |
| `MapNew { dest, entries }` | `v0 = %{v1: v2}` | Consumes all keys and values |
| `TensorNew { dest, shape, elements }` | `v0 = [\| v1 v2, v3 v4 \|]` | Consumes all elements |
| `TableNew { dest, rows }` | `v0 = table [v1, v2]` | Consumes all rows |

A collection of a dynamic type takes its descriptor from the function's
descriptor table, as the type of `dest` (Section 11).

A tensor's elements are given in row-major order, and the shape is what says how
to group them. The printed form reproduces the source's multi-comma layout, with
a trailing comma run where the outermost extent is 1, so that the rank survives.

### Slots, Parameters and References

| Instruction | Printed | Ownership |
|---|---|---|
| `SlotStoreCopy { dest, value }` | `store.copy s0, v1` | Borrows `value` |
| `SlotStoreCopyTracked { dest, value }` | `store.copy.tracked s0, v1` | Borrows; writes LIVE |
| `SlotStoreMove { dest, value }` | `store.move s0, v1` | Consumes `value` |
| `SlotStoreMoveTracked { dest, value }` | `store.move.tracked s0, v1` | Consumes; writes LIVE |
| `SetField { slot, field_path, value }` | `setfield s0.1.0, v1` | Consumes `value` |
| `SetFieldTracked { slot, field_path, value }` | `setfield.tracked s0.1, v1` | Consumes; writes LIVE |
| `ParamStore { param, value }` | `store p0, v1` | Consumes |
| `ParamStoreTracked { param, value }` | `store.tracked p0, v1` | Consumes; writes LIVE |
| `ParamSetField { param, field_path, value }` | `setfield p0.1, v1` | Consumes |
| `ParamSetFieldTracked { param, field_path, value }` | `setfield.tracked p0.1, v1` | Consumes; writes LIVE |
| `RefStore { dest, value }` | `refstore v0, v1` | Consumes |
| `RefStoreTracked { dest, value }` | `refstore.tracked v0, v1` | Consumes; writes LIVE |
| `RefSetField { dest, field_path, value }` | `refsetfield v0.1, v1` | Consumes |
| `RefSetFieldTracked { dest, field_path, value }` | `refsetfield.tracked v0.1, v1` | Consumes; writes LIVE |
| `SlotLoadCopy { dest, slot }` | `v0 = load.copy s0` | Borrows the slot |
| `SlotLoadMove { dest, slot }` | `v0 = load.move s0` | Consumes the slot's contents |
| `SlotLoadMoveTracked { dest, slot }` | `v0 = load.move.tracked s0` | Consumes; writes MOVED |

A `field_path` is the chain of field indices from the root to the target, so
`set a.x.0.y = v` carries three of them.

None of these destroys what was at the destination; a drop in front of the
store does, when the type owns something.

### Drops

| Instruction | Printed | Ownership |
|---|---|---|
| `Drop { operand }` | `drop v0` | Consumes |
| `DropTracked { operand }` | `drop.tracked s0` | Consumes if the tracking byte says LIVE |
| `DropViaRef { ref_value }` | `drop.ref v0` | Destroys the referent, not the reference |
| `DropField { base, field_path }` | `drop.field s0.1` | Destroys the field of a slot, parameter or referent; not `base` |
| `UnitEndDrop { operand }` | `unit_end_drop v0` | See below |
| `UnitEndDropTracked { operand }` | `unit_end_drop.tracked s0` | See below |

The two unit-end forms are where the interpreter and the AOT backends
deliberately differ: the interpreter treats them as no-ops, because a REPL
binding has to survive the line that made it, and the AOT backends drop.

### List, Map and Tensor Access

| Instruction | Printed | Ownership |
|---|---|---|
| `ListGet { dest, is_valid, list, index }` | `v0, v1 = listget v2[v3]` | Borrows both; `dest` valid only if `is_valid` |
| `ListBoundsCheck { is_valid, list, index }` | `v0 = listboundscheck v1[v2]` | Borrows both |
| `ListSet { is_valid, list, index, value }` | `v0 = listset v1[v2] = v3` | Mutably borrows the list, consumes `value` whatever the outcome |
| `ListElementRef { dest, list, index }` | `v0 = listelementref v1[v2]` | Borrows; produces a `Ref` |
| `MapGet { dest, is_valid, map, key }` | `v0, v1 = mapget v2[v3]` | Borrows both |
| `MapContainsKey { is_valid, map, key }` | `v0 = mapcontainskey v1[v2]` | Borrows both |
| `MapSetValue { is_valid, map, key, value }` | `v0 = mapsetvalue v1[v2] = v3` | Consumes `value` whatever the outcome |
| `MapValueRef { dest, map, key }` | `v0 = mapvalueref v1[v2]` | Key must exist; produces a `Ref` |
| `MapUpsert { map, key, value }` | `mapupsert v0[v1] = v2` | Consumes key and value; always succeeds |
| `TensorGet { dest, is_valid, tensor, index }` | `v0, v1 = tensorget v2[v3]` | Borrows both |
| `TensorBoundsCheck { is_valid, tensor, index }` | `v0 = tensorboundscheck v1[v2]` | Borrows both |
| `TensorSet { is_valid, tensor, index, value }` | `v0 = tensorset v1[v2] = v3` | Rank 1 only; consumes `value` whatever the outcome |
| `TensorIndexRef { dest, tensor, index }` | `v0 = tensorindexref v1[v2]` | Borrows; produces a `Ref` |

The reference-producing forms assume the bounds or the key have already been
checked, by the matching check instruction: the source's `a[i]?.f` lowers to a
check, a branch to the early-return path, and then a reference that cannot
fail. A reference into a collection is a borrowed place, and nothing is
half-done between the check and it.

The `Set` forms check for themselves, as the runtime's own `list_set` does:
one operation that either replaces the element and sets `is_valid`, or leaves
the collection as it was, destroys the value and clears it. `set a[i]? = v`
lowers to the write and a branch on its `is_valid`. Splitting a write into a
check, or a drop of the old element, and a store would leave a state between
the two that the runtime does not know about. How a backend implements the one
operation -- through the runtime, or with the check and a store inline where
the types allow -- is its own business.

`TensorGet` at rank 1 clones the element; at higher ranks it produces an owned
sub-tensor. `TensorIndexRef` at rank 1 points at the element; at higher ranks it
builds a view -- a tensor struct with capacity zero, aliasing the parent's
buffer -- on the stack and points at that.

`MapUpsert` is the one collection write with no check before it: it inserts when
the key is absent and overwrites when it is present, destroying the old value
and the key it was handed. That is the source's bare `set m[key] = v`.

### Miscellaneous

| Instruction | Printed | Ownership |
|---|---|---|
| `DebugLog { operand }` | `debuglog v0` | Borrows |
| `Intrinsic { dest, intrinsic, args }` | `v0 = intrinsic ClzU32(v1)` | Per the intrinsic |
| `Nop` | `nop` | |

An `Intrinsic` compiles to a single machine operation with no call overhead.
`datalove-datafun-intrinsics` defines `IntrinsicId` with stable discriminants
for serialization, numbered in blocks by family: bitwise, shifts, bit counting,
byte manipulation, wrapping arithmetic, type conversion, platform queries, and
then a block per numeric type. The interpreter implements them in
`interp/src/intrinsics.rs`; the compiled backends emit instructions directly.

## 8. Terminators

```rust
pub enum Terminator {
    Goto { target, args },
    Branch { cond, then_block, then_args, else_block, else_args },
    Switch { discriminant, cases, default },
    Return { value: Option<Operand> },
    UnitEnd { result: Option<Operand> },
    UnitEarlyReturn { value: Operand },
}
```

| Terminator | Printed |
|---|---|
| `Goto` | `goto block1(v0)` |
| `Branch` | `branch v0, block1(v1), block2` |
| `Switch` | `switch v0, [0 => block1, 1 => block2], default => block3` |
| `Return` | `return v0` or `return` |
| `UnitEnd` | `unit_end v0` or `unit_end` |
| `UnitEarlyReturn` | `unit_early_return v0` |

`Goto` and `Branch` **move** their arguments into the target block's
parameters. `Switch` takes no arguments and maps straight onto a jump table.

`Return` leaves a function. `UnitEnd` ends a script unit, optionally with a
result to print or bind. `UnitEarlyReturn` is a script unit leaving early --
from a `ret`, from `!` or `?`, or from a checked operator that overflowed.

## 9. Ownership and Tracking

Ownership analysis runs after typechecking and hands lowering a `DropSchedule`,
which says where each binding is destroyed: at a branch exit, before a return,
before a try-return, before a set-target early return, at a loop body's end,
before a `break` or `continue`, or at a match arm's exit. Lowering emits the
drops the schedule names; a backend adds none of its own.

Bindings fall into three categories:

- **Copy** -- nothing to track and nothing to drop.
- **Precise** -- the state is known statically, so the instruction can say
  outright what happens.
- **Tracked** -- the state may vary by the path taken, so a tracking byte in the
  frame carries it: `UNINIT = 0x00`, `LIVE = 0x01`, `MOVED = 0x02`.

Tracked bindings are the exports, the `out` parameters, the conditionally-moved
values and the mutable slots. `tracked_slots` on the code unit and
`tracked_params` on the function context list them.

This is what the `Tracked` instruction variants are for. A precise variant
assumes ownership analysis has proven the operand's state and neither reads nor
writes a tracking byte. A tracked variant reads the byte before destroying
anything and writes it afterwards.

The full error catalogue (D001 to D013) is in the
[compiler guide](compiler-guide.md#user-content-error-codes).

## 10. Constants

`ConstValue` is a compile-time value, and covers every datafun type, because
compile-time function evaluation runs the same IR the run time does. Beyond the
scalars and `String` it has `Tuple`, `Struct`, `Enum`, `OptionSome`,
`OptionNone`, `ResultOk`, `ResultErr`, `Data`, `Error`, `List`, `Set`, `Map`,
`Tensor` and `Table`.

Three of them keep more than the value:

- `Int { limbs, negative }` stores a bigint as little-endian base-2^32 limbs,
  with empty limbs meaning zero.
- `Data { payload_type, value }` and `Error { payload_type, value }` keep the
  type of what they hold, because a `data` carries its own descriptor at run
  time and the value alone cannot always say what that should be: an empty map
  says `%{() = ()}`, which is not a type the program has and which no backend
  has a descriptor for.
- `Tensor { shape, elements }` keeps the shape, because nine numbers are a 3 by
  3 or a 9 by 1 depending only on that. The rank belongs to the type; the
  extents belong to the value.

`ir_type_of_const_value` reads the type back off a value. Where a value cannot
say the whole of a type it answers with the part it can -- an enum gives the one
variant it holds, a `none` gives `?()` -- which is enough for the two things it
is for, making a descriptor for a `data` and sizing a buffer to build one in,
and is not enough to check anything against.

### Float constants

`ConstF32` and `ConstF64` wrap the primitives and compare and hash **by bits**.
That is deliberately not the `==` operator's relation. It is IEEE 754-2008
`totalOrder`, the one sets and maps keep their keys in, which gives NaN a place
and tells the two zeros apart.

Asking a constant whether it is the same constant is that question and not the
operator's. A `ConstValue` is what decides whether two call sites name one
instantiation, and what salsa compares to decide whether lowering can be reused.
Under the operator's relation a NaN constant is not equal to itself, which makes
`Eq` a lie and stops any of that working. The wrapper carries `serde(transparent)`,
so what is written down did not change when the relation did.

### CTFE

```rust
pub trait CtfeEvaluator {
    fn evaluate(&mut self, unit: &IrCodeUnit, result_type: &IrType)
        -> Result<ConstValue, CtfeError>;
    fn set_module_registry(&mut self, registry: Arc<ModuleFunctionRegistry>);
}
```

The interpreter implements it, and is the only thing that does. The registry is what lets a
const expression call a function in another module, and it holds the riders'
native units as well, so a const can call a native.

Calling one needs its code, which the evaluator's interpreter finds through a
`NativeResolver` (`InterpCtfeEvaluator::with_native_resolver`). The pipeline is
given one with `set_natives` and builds every evaluator with it. A driver gives
it a `RiderNatives`, the same one it later registers the executor's natives
from: built riders are compiled and loaded the first time either asks for a
symbol, and only then, and the riders linked into the binary are the special
case that looks the address up instead.

Around it sit the memoization types: `ConstStmtId` numbers const statements in
source order, `ConstBindingInfo` and `ConstBindingGraph` are the collection
phase's output in dependency order, and `ResolvedConsts` is the evaluation
phase's. All of them use sorted vectors rather than hash maps so that hashing is
deterministic, which is what salsa needs. A resolved value is an
`Arc<ConstValue>`, here and in every map of consts after it, down to the
`StaticRef` a reference lowers to, so a const is one value however many
functions and references hold it.

`ConstStmtId` used to be a `salsa::Id`, on the grounds that an opaque id avoided
coupling the IR to AST types. It coupled it to something worse, for the reason
given in Section 2.

Two error types: `CtfeError` for interpreter-level failures, and
`ConstEvalError` for the phase above -- gas expiry, a failed dependency, a
lowering failure, an early return, an unsupported type.

## 11. Generics and Descriptors

A generic function is compiled **once**. Its types are as written, with
`IrType::Param(i)` for its `i`th type parameter, and a value of a type that
mentions one -- a *dynamic* type, `IrType::is_dynamic` -- is a pointer at
the value in its real layout, described by the function's descriptor table
entry for its type. [Generics: how it
works](generics.md) is the whole story; this is what the IR carries for it.

### In the function

- `FunctionContext::type_param_count` is how many type parameters the
  function was declared with. It takes one descriptor per type parameter after
  its ordinary parameters. The count is the signature's, never the body's.
- `FunctionContext::descriptor_table` is every dynamic type the body uses,
  each written over the function's own parameters, parts before what holds
  them. The function fills a table of their descriptors at entry, and any
  instruction that needs a dynamic type's descriptor reads its entry.
  `descriptor_table(unit)` computes it, from the parameters, return type,
  values, slots and the calls' bindings, so every backend and every rewrite
  agrees on the order.

`uses_descriptors(unit)` is whether a unit is generic or calls one.

### At a call

`Call::bindings` and `ComptimeCall::bindings` say what the site bound each of
the callee's type parameters to, in the callee's declaration order and written
over the caller's own type parameters. Each becomes a descriptor handed over:
a static one when the binding is concrete, the caller's table entry when it
mentions the caller's parameters. A native call hands over only the bindings
at its `undetermined_type_params`.

Nothing about a callee's descriptors comes from its body, so a call at a
growing type -- `f<T>` calling `f<[T]>` -- needs nothing ahead of time; it
builds one more descriptor per level at run time.

### Block arguments

A jump that passes a block's own parameters back to it permuted would make a
byte-by-byte copy of dynamic block arguments read storage it already wrote.
Lowering never emits one, and checks it (`permuted_block_args`).

### Dynamic operators

`dyn_op_code` maps a `BinOp` to the runtime's `DynOp` for an operator applied to
a value whose type only a descriptor says, and gives `None` for an operator no
bound admits. One function, read by every backend, so that what they pass and
what the runtime reads cannot drift apart.

## 12. Registries

```rust
pub struct ModuleFunctionRegistry { /* BTreeMap<(IrModuleId, CodeUnitId), IrCodeUnit> */ }
pub struct UnitFunctionRegistry   { /* Vec<Vec<IrCodeUnit>> */ }
pub struct FunctionRegistry       { module: Arc<ModuleFunctionRegistry>, unit: UnitFunctionRegistry }
```

`ModuleFunctionRegistry` holds compiled module functions and is immutable once
module compilation finishes, so it is shared behind an `Arc`. It is a `BTreeMap`
and not a hash map because the backends walk it to declare functions, and the
declaration order decides the identifiers and the layout of the object file; a
hash map made that order depend on the seed the process started with, and the
same input produced different bytes on every run.

`UnitFunctionRegistry` holds each script unit's functions and grows as units
execute. `unit_count()` is both the number of finished units and the index of
the one now running, since a unit is added once it has finished.

`FunctionRegistry` is the combined view, and is what a backend is usually handed.

## 13. Passes

Everything here transforms `IrCodeUnit` in place or produces a new one.

**Const inlining and DCE** (`datalove-datafun-const`) evaluates a const
binding's pre-lowered IR to a `ConstValue`, replaces the expression with a
`Const` instruction, and then eliminates the instructions and blocks that leaves
unreachable. `dce::instruction_dest` is the shared answer to "what does this
instruction define".

**Inlining** (`ir::inline`, temporary) replaces each call to a function marked
`inline` by its body, after specialization: the callee's blocks renumbered in
after the caller's, the call's block ending in a jump to them, and every
return a jump to a continuation holding what followed the call, with the
result as its parameter. An `in` parameter is moved or copied into a value of
its own, a `ref` or `mut` one is the argument, and an `out` one is a tracked
slot moved into the argument's place at the continuation. A generic callee's
types take the call's bindings. The blocks are then put in reverse postorder,
which the compiled backends need to meet a definition before its uses. A
cycle of inline functions is an error.

**Const parameter specialization** monomorphizes: the original function stays
and a copy is added beside it per instantiation, with the const parameter
replaced by its value throughout. The instantiations are read out of the IR, off
the `ComptimeCall` instructions.

**Parameter substitution** (`datalove_datafun_ir::params`) is the primitive both
of those need: `replace_params_in_instruction` and `replace_params_in_terminator`
rewrite `Operand::Param` occurrences. The match over instructions is exhaustive
on purpose -- a variant falling through unsubstituted leaves a reference to a
parameter the transformed function may no longer have, and nothing downstream
reports that.

**Specialization and generics.** A specialized copy keeps its type
parameters, its `type_param_count` and its descriptor table; it drops only the
const parameters, which come before the descriptors.

## 14. Consumers

All four agree with `ir::layout`.

- **Interpreter** (`datalove-datafun-interp`) walks the IR directly. Its values
  are a pointer and a descriptor already; a generic's frame holds its
  descriptor table and the pointers of its dynamic values.
- **Cranelift AOT** (`datalove-datafun-cranelift-aot`) emits an object file.
- **C backend** (`datalove-datafun-c-aot`) emits C and compiles it. The same
  program by a different route, which is why the `dual` fixtures run both.
- **JIT** (`datalove-datafun-cranelift-jit`) compiles hot functions at run time.
  The `OptimizingDispatcher` counts calls and runs compiled code once a
  function is compiled.

## 15. Text and Serialized Forms

There are two, and they are not interchangeable.

**The printed form** comes from the `Display` impls in `display.rs`. It is what
the `ir_lower` fixtures compare, and what `datalove script-ir` prints. It is
lossy on purpose: value types, tracked sets, descriptor tables and a call's
`bindings` are not shown, because a fixture that reprinted all of them would
change whenever any of them did.

The conventions: `v0` for a value, `s0` for a slot, `p0` for a parameter,
`block0` for a block, `u0` for a code unit, `m0` for a module, `f0` for a
`FuncId`, `*v0` for a `ValueRef`, `unit1.v0` for something in an earlier script
unit, and `@3` for a call site id. A constant prints with its type as a suffix:
`42int`, `1u8`, `1.5f64`.

```text
fn main():
block0:
    v0 = const true
    branch v0, block1, block2
block1:
    v1 = const 1i64
    return v1
block2:
    v2 = const 0i64
    return v2
```

A script unit prints as `scriptunit:` followed by its blocks and then its nested
functions; a native prints as `native fn NAME -> symbol SYMBOL`.

**The serialized form** is RON, via serde, and is complete.
`IrCodeUnit::to_ron` / `from_ron` and `FunctionRegistry::to_ron` / `from_ron`
are the entry points, and the `ir_serial` fixtures round-trip a unit and then
check that both the interpreter and the AOT backend produce the same output from
the deserialized copy as from the original.

Several fields carry `#[serde(default)]` -- `tracked_slots`,
`const_values`, `nested_units`, `tracked_params`, a call's `bindings`, and a
script context's `unit_end_values`,
`unit_end_slots`, `result_name` and `exports` -- so that an older serialization
still reads.

`expand_ir_strings` rewrites an `ir: "..."` field in RON output into a
triple-quoted multiline block, which is what makes the fixture files readable
without changing what is compared.

## 16. Invariants

What every phase producing IR has to maintain, and every phase consuming it may
assume.

1. `blocks[0]` is the entry block.
2. Every block ends in exactly one terminator, and has no terminator before it.
3. A `ValueId` is assigned exactly once. A block parameter counts as an
   assignment, at block entry.
4. `value_types` has `value_count` entries and is indexed by `ValueId`;
   `slot_types` has `slot_count` entries and is indexed by `SlotId`.
5. A `Goto` or `Branch` passes exactly as many arguments as the target block
   declares parameters, of matching types.
6. A linear value is consumed on every path, exactly once, either by an
   instruction that consumes it or by a `Drop`.
7. A tracked destination is written only by a `Tracked` variant, and a precise
   one only by a precise variant.
8. An access instruction that assumes a check -- `MapValueRef` and the
   element-reference forms -- is dominated by the matching check instruction
   and by the branch that acted on it.
9. No jump passes a block's own parameters back to it permuted.
10. An enum's `variant_index` indexes the type's variants **sorted by name**.
11. A `Table`'s columns are in written order, everywhere, in the type and in the
    value alike.
12. Every dynamic type an instruction needs the descriptor of is in the
    enclosing function's `descriptor_table`.
13. A `Call`'s `bindings` has one entry per type parameter of the callee, and
    is empty when the callee has none.

## 17. Tests

| Suite | What it covers |
|---|---|
| `ir_lower_tests` | IR lowering from worldfiles |
| `ir_lower_script_tests` | Script IR lowering |
| `ir_serial_tests` | Serialization round-trip, and that both backends agree across it |
| `layout_conformance_tests` | `ir::layout` against `rtdt::layout`, both directions |
| `aot_layout_tests` | The cranelift AOT type layouts against the interpreter's type descriptors |
| `engine_tests` | Every engine, and the program compiled without specialization or const inlining, agreeing with the IR walker |

`layout_conformance_tests` is the one worth naming twice. It walks a corpus of
types and checks size, alignment, field offsets, variant payload offsets and tag
payload offsets in both directions, because the failure it guards against is a
wrong read rather than a failed compile.

Bless expected output with `BLESS=1 cargo test`.

## See Also

- [Compiler Guide](compiler-guide.md) -- the pipeline this sits in the middle of
- [Language Specification](botspec.md) -- what the source means
- [The Native Rider ABI](native-abi.md) -- how a native call is made
- [Generics: how it works](generics.md) -- dynamic values and descriptor tables
- [Const Parameter Specialization](compiler-guide.md#user-content-const-parameter-specialization)
  -- monomorphization
- [IrCodeUnit](compiler-guide.md#user-content-ircodeunit) -- one type for all
  three kinds of code unit
- [index-64](compiler-guide.md#user-content-index-64) -- what decides the width
  of `Index`
