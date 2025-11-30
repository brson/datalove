# Plan: Complete Binary Operations and Literals Implementation

## Goal

Implement complete binary operations and comparison operators for all numeric types per the botspec.

## Current State

**Implemented:**
- Bare arithmetic (`+ - * /`): u32 and int only, widens u32 to int
- Checked arithmetic (`+! -! *! /!`): u32 only

**Not Implemented:**
- f32 bare arithmetic
- Other fixed int types (i8, i16, i32, i64, u8, u16, u64)
- Optional arithmetic (`+? -? *? /?`)
- Comparison operators (`.<` `.>` `<=` `>=` `==` `!=`)
- int division (`/!` and `/?` for bigint)

## Infrastructure Already Available

- `dtlv_rti_cmp_total` - Runtime comparison function
- `dtlv_rti_int_div_checked` - Bigint division with div-by-zero check
- `instantiate_f32` - f32 literal instantiation
- `instantiate_option` - Option value creation (None/Some)
- `TyDescTable::create_option_from_inner_tydesc` - Create Option type descriptors

## Spec Summary (from botspec.md)

### Bare Arithmetic (`+ - * /`)
| Type | `+` `-` `*` | `/` |
|------|-------------|-----|
| f32 | Returns f32 | Returns f32 |
| int | Returns int | **Not allowed** |
| Fixed ints | Widen to int | **Not allowed** |

### Checked Arithmetic (`+! -! *! /!`)
| Type | `+!` `-!` `*!` | `/!` |
|------|----------------|------|
| f32 | Not allowed | Not allowed |
| int | Not allowed | Returns int (early-return on div0) |
| Fixed ints | Returns same type (early-return on overflow) | Returns same type |

### Optional Arithmetic (`+? -? *? /?`)
| Type | `+?` `-?` `*?` | `/?` |
|------|----------------|------|
| f32 | Not allowed | Not allowed |
| int | Not allowed | Returns ?int (returns none on div0) |
| Fixed ints | Returns ?T (returns none on overflow) | Returns ?T |

### Comparison (`.<` `.>` `<=` `>=` `==` `!=`)
- Returns `bool` for any numeric operands

## Implementation Plan

### Phase 1: Comparison Operators

**Add `eval_comparison` function:**
```rust
fn eval_comparison<'db>(
    ctx: &mut InterpContext<'db>,
    op: ast::BinOp,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    let ordering = unsafe {
        datalove_rt::c::dtlv_rti_cmp_total(
            ctx.runtime.handle(),
            lhs.ptr,
            lhs.tydesc,
            rhs.ptr,
            rhs.tydesc,
        )
    };

    destroy_value(ctx, lhs);
    destroy_value(ctx, rhs);

    let result = match (op, ordering) {
        (BinOp::Lt, RtOrdering::Less) => true,
        (BinOp::Gt, RtOrdering::Greater) => true,
        (BinOp::Le, RtOrdering::Less | RtOrdering::Equal) => true,
        (BinOp::Ge, RtOrdering::Greater | RtOrdering::Equal) => true,
        (BinOp::Eq, RtOrdering::Equal) => true,
        (BinOp::Ne, RtOrdering::Less | RtOrdering::Greater) => true,
        (_, RtOrdering::Error) => return Err(InterpError::RuntimeError("Comparison failed".into())),
        _ => false,
    };

    allocate_bool(ctx, result)
}
```

**Update `execute_binop`:**
```rust
BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
    eval_comparison(ctx, op, lhs, rhs, dest)
}
```

### Phase 2: f32 Bare Arithmetic

**Add type detection:**
```rust
fn is_f32_value(value: Value) -> bool {
    unsafe { (*value.tydesc).type_tag() == rtdt::TyTag::F32 }
}
```

**Add allocation:**
```rust
fn allocate_f32<'db>(ctx: &mut InterpContext<'db>, value: f32) -> Result<Value, InterpError> {
    let tydesc = ctx.tydesc_table.get_or_create(&datalit::tycheck::Type::F32);
    let size = std::mem::size_of::<f32>();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            ctx.runtime.handle(), size, std::mem::align_of::<f32>(), 1
        )
    };
    if ptr.is_null() {
        return Err(InterpError::AllocationFailed);
    }
    unsafe { *(ptr as *mut f32) = value; }
    Ok(Value { ptr, tydesc, location: ValueLocation::TempOwned })
}
```

**Extend `eval_add/sub/mul/div` for f32:**
```rust
// In eval_add:
if is_f32_value(lhs) && is_f32_value(rhs) {
    let a = unsafe { *(lhs.ptr as *const f32) };
    let b = unsafe { *(rhs.ptr as *const f32) };
    destroy_value(ctx, lhs);
    destroy_value(ctx, rhs);
    return allocate_f32(ctx, a + b);
}
```

### Phase 3: int Division (`/!` and `/?`)

**Extend `eval_div_checked` for int:**
```rust
// Add to eval_div_checked after u32 handling:
if is_int_value(lhs) && is_int_value(rhs) {
    let result_int = allocate_bigint(ctx)?;
    let status = unsafe {
        datalove_rt::c::dtlv_rti_int_div_checked(
            ctx.runtime.handle(),
            lhs.ptr, lhs.tydesc,
            rhs.ptr, rhs.tydesc,
            result_int.ptr, result_int.tydesc,
        )
    };
    destroy_value(ctx, lhs);
    destroy_value(ctx, rhs);
    if status == RtStatus::Error {
        destroy_value(ctx, result_int);
        return Err(InterpError::DivisionByZero);
    }
    return Ok(result_int);
}
```

**Add `eval_div_optional` for int:**
```rust
fn eval_div_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    if is_int_value(lhs) && is_int_value(rhs) {
        // Try division
        let result_int = allocate_bigint(ctx)?;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_div_checked(...)
        };

        if status == RtStatus::Error {
            // Return None
            destroy_value(ctx, result_int);
            return allocate_option_none(ctx, &datalit::tycheck::Type::Int);
        }
        // Return Some(result)
        return allocate_option_some_from_value(ctx, result_int);
    }
    // ... u32 handling with checked_div
}
```

### Phase 4: Optional Arithmetic Helpers

**Add Option allocation helpers:**
```rust
fn allocate_option_none<'db>(
    ctx: &mut InterpContext<'db>,
    inner_type: &datalit::tycheck::Type<'db>,
) -> Result<Value, InterpError> {
    let inner_tydesc = ctx.tydesc_table.get_or_create(inner_type);
    let option_tydesc = ctx.tydesc_table.create_option_from_inner_tydesc(inner_tydesc);
    let layout = unsafe { rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(option_tydesc)) };

    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            ctx.runtime.handle(),
            layout.total_size as usize,
            layout.align as usize,
            1
        )
    };
    if ptr.is_null() {
        return Err(InterpError::AllocationFailed);
    }

    unsafe { *ptr = rtdt::OptionTag::None as u8; }
    Ok(Value { ptr, tydesc: option_tydesc, location: ValueLocation::TempOwned })
}

fn allocate_option_some_from_value<'db>(
    ctx: &mut InterpContext<'db>,
    inner_value: Value,
) -> Result<Value, InterpError> {
    let option_tydesc = ctx.tydesc_table.create_option_from_inner_tydesc(inner_value.tydesc);
    let layout = unsafe { rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(option_tydesc)) };

    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            ctx.runtime.handle(),
            layout.total_size as usize,
            layout.align as usize,
            1
        )
    };
    if ptr.is_null() {
        return Err(InterpError::AllocationFailed);
    }

    unsafe {
        *ptr = rtdt::OptionTag::Some as u8;
        let payload_ptr = ptr.add(layout.payload_offset as usize);
        let inner_size = (*inner_value.tydesc).size as usize;
        std::ptr::copy_nonoverlapping(inner_value.ptr, payload_ptr, inner_size);
    }

    // Free the inner value's container (if TempOwned), but data is now in Option
    if inner_value.location == ValueLocation::TempOwned {
        free_value_structure(ctx, inner_value);
    }

    Ok(Value { ptr, tydesc: option_tydesc, location: ValueLocation::TempOwned })
}
```

### Phase 5: Optional Arithmetic for Fixed Ints

**Implement `eval_add_optional`, `eval_sub_optional`, `eval_mul_optional`:**
```rust
fn eval_add_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    if is_u32_value(lhs) && is_u32_value(rhs) {
        let a = unsafe { *(lhs.ptr as *const u32) };
        let b = unsafe { *(rhs.ptr as *const u32) };
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        match a.checked_add(b) {
            Some(result) => {
                let val = allocate_int(ctx, result)?;
                allocate_option_some_from_value(ctx, val)
            }
            None => allocate_option_none(ctx, &datalit::tycheck::Type::U32),
        }
    } else {
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        Err(InterpError::InvalidExpression("Optional addition requires matching fixed int types".into()))
    }
}
```

## Files to Modify

- `crates/datalove-datafun/src/interp/mod.rs`:
  - Add `eval_comparison()`
  - Add `is_f32_value()`, `allocate_f32()`
  - Extend `eval_add/sub/mul/div` for f32
  - Add `eval_div_checked` for int
  - Add `eval_*_optional` functions
  - Add `allocate_option_none`, `allocate_option_some_from_value`

## Testing

Add test cases in `tests/fixtures/interp2/`:
- `37_cmp_u32.world` - u32 comparisons
- `38_cmp_int.world` - int comparisons
- `39_f32_arithmetic.world` - f32 +, -, *, /
- `40_int_div_checked.world` - int /!
- `41_optional_arithmetic.world` - +?, -?, *?, /?

## Implementation Order

1. Comparison operators (quick win, no new infrastructure)
2. f32 bare arithmetic (simple addition)
3. int /! division (uses existing runtime function)
4. Option allocation helpers
5. Optional arithmetic for u32
6. Extend to other fixed int types as needed
