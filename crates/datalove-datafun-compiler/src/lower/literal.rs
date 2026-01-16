//! Literal parsing for IR lowering.
//!
//! Converts source text literals to IR constant values.

use datalove_datafun_ir::{IrType, ConstValue};

/// Convert a decimal string to bigint limbs (little-endian base 2^32).
pub fn parse_decimal_to_limbs(text: &str) -> Result<(Vec<u32>, bool), ()> {
    let (negative, text) = if let Some(rest) = text.strip_prefix('-') {
        (true, rest)
    } else {
        (false, text)
    };

    // Parse digit by digit, multiply by 10 and add.
    let mut limbs: Vec<u32> = vec![0];

    for c in text.chars() {
        let digit = c.to_digit(10).ok_or(())?;

        // Multiply all limbs by 10.
        let mut carry: u64 = 0;
        for limb in &mut limbs {
            let product = (*limb as u64) * 10 + carry;
            *limb = product as u32;
            carry = product >> 32;
        }
        if carry > 0 {
            limbs.push(carry as u32);
        }

        // Add the digit.
        let mut add_carry: u64 = digit as u64;
        for limb in &mut limbs {
            let sum = (*limb as u64) + add_carry;
            *limb = sum as u32;
            add_carry = sum >> 32;
            if add_carry == 0 {
                break;
            }
        }
        if add_carry > 0 {
            limbs.push(add_carry as u32);
        }
    }

    // Remove trailing zeros from the end (high-order limbs).
    while limbs.len() > 1 && limbs.last() == Some(&0) {
        limbs.pop();
    }

    // Handle zero case.
    if limbs.len() == 1 && limbs[0] == 0 {
        limbs.clear();
    }

    let is_nonzero = !limbs.is_empty();
    Ok((limbs, negative && is_nonzero))
}

/// Convert a hex string to bigint limbs (little-endian base 2^32).
pub fn parse_hex_to_limbs(hex_str: &str) -> Result<(Vec<u32>, bool), ()> {
    // Parse from right to left, 8 hex digits at a time = 1 u32 limb.
    let mut limbs = Vec::new();
    let len = hex_str.len();

    let mut i = len;
    while i > 0 {
        let start = if i >= 8 { i - 8 } else { 0 };
        let chunk = &hex_str[start..i];
        let limb = u32::from_str_radix(chunk, 16).map_err(|_| ())?;
        limbs.push(limb);
        i = start;
    }

    // Remove trailing zeros.
    while limbs.len() > 1 && limbs.last() == Some(&0) {
        limbs.pop();
    }

    // Handle zero case.
    if limbs.len() == 1 && limbs[0] == 0 {
        limbs.clear();
    }

    // Hex is always non-negative for now.
    Ok((limbs, false))
}

/// Parse an integer literal into a ConstValue based on the target type.
pub fn parse_int_const(text: &str, ty: &IrType) -> Result<ConstValue, ()> {
    match ty {
        IrType::U8 => text.parse::<u8>().map(ConstValue::U8).map_err(|_| ()),
        IrType::U16 => text.parse::<u16>().map(ConstValue::U16).map_err(|_| ()),
        IrType::U32 => text.parse::<u32>().map(ConstValue::U32).map_err(|_| ()),
        IrType::U64 => text.parse::<u64>().map(ConstValue::U64).map_err(|_| ()),
        IrType::I8 => text.parse::<i8>().map(ConstValue::I8).map_err(|_| ()),
        IrType::I16 => text.parse::<i16>().map(ConstValue::I16).map_err(|_| ()),
        IrType::I32 => text.parse::<i32>().map(ConstValue::I32).map_err(|_| ()),
        IrType::I64 => text.parse::<i64>().map(ConstValue::I64).map_err(|_| ()),
        IrType::Int => {
            let (limbs, negative) = parse_decimal_to_limbs(text)?;
            Ok(ConstValue::Int { limbs, negative })
        }
        _ => Err(()),
    }
}

/// Parse a float literal into a ConstValue based on the target type.
pub fn parse_float_const(text: &str, ty: &IrType) -> Result<ConstValue, ()> {
    match ty {
        IrType::F32 => text.parse::<f32>().map(ConstValue::F32).map_err(|_| ()),
        IrType::F64 => text.parse::<f64>().map(ConstValue::F64).map_err(|_| ()),
        _ => Err(()),
    }
}

/// Parse a hex literal into a ConstValue based on the target type.
pub fn parse_hex_const(hex_str: &str, ty: &IrType) -> Result<ConstValue, ()> {
    match ty {
        IrType::U8 => u8::from_str_radix(hex_str, 16).map(ConstValue::U8).map_err(|_| ()),
        IrType::U16 => u16::from_str_radix(hex_str, 16).map(ConstValue::U16).map_err(|_| ()),
        IrType::U32 => u32::from_str_radix(hex_str, 16).map(ConstValue::U32).map_err(|_| ()),
        IrType::U64 => u64::from_str_radix(hex_str, 16).map(ConstValue::U64).map_err(|_| ()),
        IrType::I8 => i8::from_str_radix(hex_str, 16).map(ConstValue::I8).map_err(|_| ()),
        IrType::I16 => i16::from_str_radix(hex_str, 16).map(ConstValue::I16).map_err(|_| ()),
        IrType::I32 => i32::from_str_radix(hex_str, 16).map(ConstValue::I32).map_err(|_| ()),
        IrType::I64 => i64::from_str_radix(hex_str, 16).map(ConstValue::I64).map_err(|_| ()),
        IrType::Int => {
            let (limbs, negative) = parse_hex_to_limbs(hex_str)?;
            Ok(ConstValue::Int { limbs, negative })
        }
        IrType::F32 => {
            // Hex value represents the bit pattern of the float.
            let bits = u32::from_str_radix(hex_str, 16).map_err(|_| ())?;
            Ok(ConstValue::F32(f32::from_bits(bits)))
        }
        IrType::F64 => {
            // Hex value represents the bit pattern of the float.
            let bits = u64::from_str_radix(hex_str, 16).map_err(|_| ())?;
            Ok(ConstValue::F64(f64::from_bits(bits)))
        }
        _ => Err(()),
    }
}
