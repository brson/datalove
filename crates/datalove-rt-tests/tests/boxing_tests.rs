//! Round-tripping values through Data.
//!
//! `data_from_local` moves a value in, `data_into_local` moves it back out
//! given the type that went in. A value has to survive the trip unchanged
//! whichever of the three forms it takes, and the trip must not leak: the
//! pointer form allocates a box that only the extract frees.

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate2};
use datalove_datalit::tydesc_table::TyDescTable;

#[salsa::tracked(returns(copy))]
fn compile<'db>(db: &'db dyn salsa::Database, source: bct::input::Source) -> datalove_datalit::tycheck::TypecheckResult<'db> {
    let parse_result = datalove_datalit::parser::parse(db, source);
    let parsed = parse_result.expr(db);
    let resolved = datalove_datalit::resolve::resolve_names(db, source, parsed.clone());
    datalove_datalit::tycheck::type_check(db, parsed, resolved)
}

fn compile_str<'db>(db: &'db Database, source_text: &str) -> AnyResult<datalove_datalit::tycheck::TypecheckResult<'db>> {
    let source = bct::input::Source::new(db, source_text.to_string());
    Ok(compile(db, source))
}

/// Move `source` into a Data and back out, and compare the bytes with the
/// original. Returns the tag the value took on the way through.
fn round_trip(source_text: &str) -> AnyResult<datalove_rtdt::anypack::Tag> {
    let db = Database::default();
    let typechecked = compile_str(&db, source_text)?;

    let rt = datalove_rt::rust::Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    let size = unsafe { (*inst.tydesc.as_ptr()).size as usize };
    let original: Vec<u8> = unsafe {
        std::slice::from_raw_parts(inst.ptr, size).to_vec()
    };

    // In.
    let mut data_buffer = datalove_rt::rust::AlignedBuffer::new(
        std::mem::size_of::<datalove_rtdt::Data>(),
    );
    let status = unsafe {
        datalove_rt::c::dtlv_rti_data_from_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc.as_ptr(),
            data_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok, "{}", source_text);

    let tag = unsafe { (*(data_buffer.as_ptr() as *const datalove_rtdt::Data)).tag() };

    // The contents moved into the Data, so the original is released without
    // being destroyed. Destroying it here would free what the Data now holds.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc.as_ptr(),
            1,
            inst.ptr as *mut u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok, "{}", source_text);

    // Out.
    let mut out_buffer = datalove_rt::rust::AlignedBuffer::new(size);
    let status = unsafe {
        datalove_rt::c::dtlv_rti_data_into_local(
            rt.handle(),
            data_buffer.as_ptr(),
            out_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok, "{}", source_text);

    let recovered: &[u8] = unsafe {
        std::slice::from_raw_parts(out_buffer.as_ptr(), size)
    };
    assert_eq!(recovered, original.as_slice(), "value changed: {}", source_text);

    // The value came back out, so destroying it is the caller's job now. The
    // Data itself is dead and must not be destroyed as well.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            out_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        );
    }

    Ok(tag)
}

#[test]
fn round_trip_small_immediates() -> AnyResult<()> {
    use datalove_rtdt::anypack::Tag;
    for source in [
        ": bool / true",
        ": u8 / 200",
        ": i8 / -100",
        ": u16 / 60000",
        ": i16 / -30000",
        ": u32 / 4000000000",
        ": i32 / -2000000000",
    ] {
        assert_eq!(round_trip(source)?, Tag::SmallImmediate, "{}", source);
    }
    Ok(())
}

#[test]
fn round_trip_inline_with_tydesc() -> AnyResult<()> {
    use datalove_rtdt::anypack::Tag;
    for source in [
        ": u64 / 18000000000000000000",
        ": i64 / -9000000000000000000",
        ": f32 / 1.5",
        ": f64 / 2.25",
    ] {
        assert_eq!(round_trip(source)?, Tag::InlineWithTyDesc, "{}", source);
    }
    Ok(())
}

#[test]
fn round_trip_heap() -> AnyResult<()> {
    use datalove_rtdt::anypack::Tag;
    for source in [
        ": int / 12345678901234567890123",
        ": string / \"hello\"",
        ": [u32] / [1, 2, 3]",
        ": (u32, u32) / (1, 2)",
    ] {
        assert_eq!(round_trip(source)?, Tag::TwoPointers, "{}", source);
    }
    Ok(())
}

/// index and offset are scalars but are not in the inline set, so they take the
/// pointer form. Recorded rather than asserted as desirable.
#[test]
fn round_trip_index_and_offset_are_boxed() -> AnyResult<()> {
    use datalove_rtdt::anypack::Tag;
    assert_eq!(round_trip(": index / 42")?, Tag::TwoPointers);
    assert_eq!(round_trip(": offset / -42")?, Tag::TwoPointers);
    Ok(())
}
