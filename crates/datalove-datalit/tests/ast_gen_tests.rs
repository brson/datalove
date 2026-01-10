use datalove_datalit::{ast_gen::*, Database};

#[test]
fn test_gen_expr_full_seeded() {
    let db = Database::default();
    let config = AstGenConfig::default();

    let _expr = gen_expr_full_seeded(&db, 42, config);
}

#[test]
fn test_gen_multiple_exprs() {
    let db = Database::default();
    let config = AstGenConfig::default();

    for seed in 0..10 {
        let _expr = gen_expr_full_seeded(&db, seed, config.clone());
    }
}

#[test]
fn test_corner_cases_strategy() {
    let db = Database::default();
    let config = AstGenConfig {
        numeric_strategy: NumericStrategy::CornerCases,
        ..Default::default()
    };

    for seed in 0..10 {
        let _expr = gen_expr_full_seeded(&db, seed, config.clone());
    }
}

#[test]
fn test_leaf_only_types() {
    let db = Database::default();
    let config = AstGenConfig {
        max_depth: 0,
        ..Default::default()
    };

    for seed in 0..10 {
        let _expr = gen_expr_full_seeded(&db, seed, config.clone());
    }
}

#[test]
fn test_random_strategy() {
    let db = Database::default();
    let config = AstGenConfig {
        numeric_strategy: NumericStrategy::Random,
        ..Default::default()
    };

    for seed in 0..10 {
        let _expr = gen_expr_full_seeded(&db, seed, config.clone());
    }
}

#[test]
fn test_generated_exprs_typecheck() {
    let db = Database::default();
    let config = AstGenConfig::default();

    for seed in 0..100 {
        let expr_full = gen_expr_full_seeded(&db, seed, config.clone());

        // Generated ASTs don't have a real source, use empty source.
        let dummy_source = bct::input::Source::new(&db, String::new());
        let resolved = datalove_datalit::resolve::resolve_names(&db, dummy_source, expr_full);
        let typechecked = datalove_datalit::tycheck::type_check(&db, expr_full, resolved);

        let has_errors = !typechecked.errors(&db).is_empty();
        let has_type = typechecked.root_type(&db).is_some();

        assert!(!has_errors && has_type,
            "Generated expression failed typecheck for seed {}: has_type={}, has_errors={}",
            seed, has_type, has_errors);
    }
}

#[test]
fn test_pretty_print_roundtrip() {
    let db = Database::default();
    let config = AstGenConfig::default();

    for seed in 0..50 {
        let expr_full = gen_expr_full_seeded(&db, seed, config.clone());

        let pretty = datalove_datalit::pretty::pretty_print(&db, expr_full);
        let pretty_clone = pretty.clone();
        let source = bct::input::Source::new(&db, pretty.into());
        let parsed = datalove_datalit::parser::parse_integration_test(&db, source);

        // Generated ASTs don't have a real source, use empty source.
        let dummy_source = bct::input::Source::new(&db, String::new());
        let resolved_orig = datalove_datalit::resolve::resolve_names(&db, dummy_source, expr_full);
        let resolved_parsed = datalove_datalit::resolve::resolve_names(&db, source, parsed);

        let typechecked_orig = datalove_datalit::tycheck::type_check(&db, expr_full, resolved_orig);
        let typechecked_parsed = datalove_datalit::tycheck::type_check(&db, parsed, resolved_parsed);

        let has_errors_orig = !typechecked_orig.errors(&db).is_empty();
        let has_errors_parsed = !typechecked_parsed.errors(&db).is_empty();

        assert!(!has_errors_orig,
            "Original expression has typecheck errors for seed {}", seed);
        if has_errors_parsed {
            eprintln!("\nSeed {}: {}", seed, pretty_clone);
            eprintln!("Error count: {}", typechecked_parsed.errors(&db).len());
            eprintln!("Original typechecked OK: {}", !has_errors_orig);

            // Check if parse matches
            let orig_pretty = datalove_datalit::pretty::pretty_print(&db, expr_full);
            let reparsed_pretty = datalove_datalit::pretty::pretty_print(&db, parsed);
            eprintln!("Original pretty: {}", orig_pretty);
            eprintln!("Reparsed pretty: {}", reparsed_pretty);
            eprintln!("Pretty-prints match: {}", orig_pretty == reparsed_pretty);

        }
        assert!(!has_errors_parsed,
            "Parsed expression has typecheck errors for seed {}:\n{}\n", seed, pretty_clone);
    }
}

#[test]
fn test_max_collection_size_enforced() {
    use datalove_datalit::ast::*;

    let db = Database::default();
    let config = AstGenConfig {
        max_collection_size: 3,
        max_depth: 2,
        type_weights: TypeWeights {
            list_type: 10,
            map_type: 10,
            set_type: 10,
            bool_type: 1,
            u32_type: 1,
            ..Default::default()
        },
        ..Default::default()
    };

    fn check_collection_size(db: &dyn salsa::Database, expr: &Expr, max_size: usize) {
        match expr {
            Expr::List(list) => {
                let elements = list.elements(db);
                assert!(elements.len() <= max_size,
                    "List has {} elements, max is {}", elements.len(), max_size);
                for elem in elements {
                    check_collection_size(db, &elem.expr(db).expr(db), max_size);
                }
            }
            Expr::Map(map) => {
                let entries = map.entries(db);
                assert!(entries.len() <= max_size,
                    "Map has {} entries, max is {}", entries.len(), max_size);
                for entry in entries {
                    check_collection_size(db, &entry.key(db).expr(db).expr(db), max_size);
                    check_collection_size(db, &entry.value(db).expr(db).expr(db), max_size);
                }
            }
            Expr::Set(set) => {
                let elements = set.elements(db);
                assert!(elements.len() <= max_size,
                    "Set has {} elements, max is {}", elements.len(), max_size);
                for elem in elements {
                    check_collection_size(db, &elem.expr(db).expr(db), max_size);
                }
            }
            Expr::Tensor(tensor) => {
                let elements = tensor.elements(db);
                assert!(elements.len() <= max_size,
                    "Tensor has {} elements, max is {}", elements.len(), max_size);
                for elem in elements {
                    check_collection_size(db, &elem.expr(db).expr(db), max_size);
                }
            }
            Expr::AnonTuple(tuple) => {
                for elem in tuple.elements(db) {
                    check_collection_size(db, &elem.expr(db).expr(db), max_size);
                }
            }
            Expr::AnonStruct(st) => {
                for field in st.fields(db) {
                    check_collection_size(db, &field.value(db).expr(db).expr(db), max_size);
                }
            }
            Expr::Data(d) => {
                check_collection_size(db, &d.value(db).expr(db).expr(db), max_size);
            }
            Expr::Error(e) => {
                check_collection_size(db, &e.value(db).expr(db).expr(db), max_size);
            }
            _ => {}
        }
    }

    for seed in 0..100 {
        let expr_full = gen_expr_full_seeded(&db, seed, config.clone());
        check_collection_size(&db, &expr_full.expr(&db).expr(&db), config.max_collection_size);
    }
}

#[test]
fn test_heap_annotation_preservation() {
    use datalove_datalit::ast::*;

    let db = Database::default();
    let config = AstGenConfig {
        heap_distribution: HeapDistribution {
            local: 10,
            global: 0,
            omitted: 0,
        },
        type_weights: TypeWeights {
            list_type: 10,
            map_type: 5,
            set_type: 5,
            u32_type: 5,
            bool_type: 5,
            ..Default::default()
        },
        max_depth: 2,
        max_collection_size: 3,
        ..Default::default()
    };

    fn check_heap_consistency(db: &dyn salsa::Database, type_hint: Option<TypeHint>, expr: &Expr) {
        if let Some(th) = type_hint {
            match (th, expr) {
                (TypeHint::List(th_list), Expr::List(list)) => {
                    let expected_heap = th_list.element_type.heap(db);
                    for elem in list.elements(db) {
                        if let Some(elem_th) = elem.type_hint(db) {
                            // Compare heap values
                            let elem_heap = elem_th.heap(db);
                            assert!(matches!((expected_heap, elem_heap),
                                (Heap::Local, Heap::Local) | (Heap::Global, Heap::Global) | (Heap::Omitted, Heap::Omitted)),
                                "List element heap should match container's element type heap");
                        }
                        check_heap_consistency(db, Some(th_list.element_type.type_hint(db)), &elem.expr(db).expr(db));
                    }
                }
                (TypeHint::Map(th_map), Expr::Map(map)) => {
                    let expected_key_heap = th_map.key_type.heap(db);
                    let expected_value_heap = th_map.value_type.heap(db);
                    for entry in map.entries(db) {
                        if let Some(key_th) = entry.key(db).type_hint(db) {
                            let key_heap = key_th.heap(db);
                            assert!(matches!((expected_key_heap, key_heap),
                                (Heap::Local, Heap::Local) | (Heap::Global, Heap::Global) | (Heap::Omitted, Heap::Omitted)),
                                "Map key heap should match container's key type heap");
                        }
                        if let Some(value_th) = entry.value(db).type_hint(db) {
                            let value_heap = value_th.heap(db);
                            assert!(matches!((expected_value_heap, value_heap),
                                (Heap::Local, Heap::Local) | (Heap::Global, Heap::Global) | (Heap::Omitted, Heap::Omitted)),
                                "Map value heap should match container's value type heap");
                        }
                        check_heap_consistency(db, Some(th_map.key_type.type_hint(db)), &entry.key(db).expr(db).expr(db));
                        check_heap_consistency(db, Some(th_map.value_type.type_hint(db)), &entry.value(db).expr(db).expr(db));
                    }
                }
                (TypeHint::Set(th_set), Expr::Set(set)) => {
                    let expected_heap = th_set.element_type.heap(db);
                    for elem in set.elements(db) {
                        if let Some(elem_th) = elem.type_hint(db) {
                            let elem_heap = elem_th.heap(db);
                            assert!(matches!((expected_heap, elem_heap),
                                (Heap::Local, Heap::Local) | (Heap::Global, Heap::Global) | (Heap::Omitted, Heap::Omitted)),
                                "Set element heap should match container's element type heap");
                        }
                        check_heap_consistency(db, Some(th_set.element_type.type_hint(db)), &elem.expr(db).expr(db));
                    }
                }
                _ => {}
            }
        }
    }

    for seed in 0..50 {
        let expr_full = gen_expr_full_seeded(&db, seed, config.clone());
        let type_hint = expr_full.type_hint(&db).map(|th| th.type_hint(&db));
        check_heap_consistency(&db, type_hint, &expr_full.expr(&db).expr(&db));
    }
}

#[test]
fn test_numeric_corner_cases_generated() {
    use datalove_datalit::ast::*;

    let db = Database::default();
    let config = AstGenConfig {
        numeric_strategy: NumericStrategy::CornerCases,
        type_weights: TypeWeights {
            f32_type: 20,
            i32_type: 20,
            u32_type: 20,
            bool_type: 1,
            ..Default::default()
        },
        max_depth: 0,
        ..Default::default()
    };

    let mut seen_pos_zero = false;
    let mut seen_neg_zero = false;
    let mut seen_i32_min = false;
    let mut seen_i32_max = false;
    let mut seen_u32_zero = false;
    let mut seen_u32_max = false;

    for seed in 0..1000 {
        let expr_full = gen_expr_full_seeded(&db, seed, config.clone());

        match expr_full.expr(&db).expr(&db) {
            Expr::Float(f) => {
                let val_str = f.value(&db).as_str(&db);
                // Note: NaN and infinity not tested because parser doesn't support them yet.
                if val_str == "0.0" || val_str == "0" {
                    seen_pos_zero = true;
                }
                if val_str == "-0.0" || val_str == "-0" {
                    seen_neg_zero = true;
                }
            }
            Expr::Int(i) => {
                let val_str = i.value(&db).as_str(&db);
                if val_str == "-2147483648" {
                    seen_i32_min = true;
                }
                if val_str == "2147483647" {
                    seen_i32_max = true;
                }
                if val_str == "0" {
                    seen_u32_zero = true;
                }
                if val_str == "4294967295" {
                    seen_u32_max = true;
                }
            }
            _ => {}
        }
    }

    assert!(seen_pos_zero, "Should generate positive zero");
    assert!(seen_neg_zero, "Should generate negative zero");
    assert!(seen_i32_min, "Should generate i32::MIN");
    assert!(seen_i32_max, "Should generate i32::MAX");
    assert!(seen_u32_zero, "Should generate u32 zero");
    assert!(seen_u32_max, "Should generate u32::MAX");
}

#[test]
fn test_type_weight_configuration() {
    use datalove_datalit::ast::*;

    let db = Database::default();
    let config = AstGenConfig {
        type_weights: TypeWeights {
            bool_type: 100,
            u8_type: 0,
            i8_type: 0,
            u16_type: 0,
            i16_type: 0,
            u32_type: 0,
            i32_type: 0,
            u64_type: 0,
            i64_type: 0,
            f32_type: 0,
            int_type: 0,
            string_type: 0,
            list_type: 0,
            map_type: 0,
            set_type: 0,
            option_type: 0,
            result_type: 0,
            tensor_type: 0,
            anon_tuple_type: 0,
            named_tuple_type: 0,
            anon_struct_type: 0,
            named_struct_type: 0,
            anon_enum_type: 0,
            named_enum_type: 0,
            data_type: 0,
            error_type: 0,
        },
        max_depth: 1,
        ..Default::default()
    };

    let mut bool_count = 0;
    let mut other_count = 0;

    for seed in 0..100 {
        let expr_full = gen_expr_full_seeded(&db, seed, config.clone());
        match expr_full.expr(&db).expr(&db) {
            Expr::True | Expr::False => bool_count += 1,
            _ => other_count += 1,
        }
    }

    assert!(bool_count >= 90, "With bool weight 100 and others 0, should generate mostly bools. Got {} bools, {} others", bool_count, other_count);
}

#[test]
fn test_result_error_case_generation() {
    use datalove_datalit::ast::*;

    let db = Database::default();
    let config = AstGenConfig {
        type_weights: TypeWeights {
            result_type: 20,
            u32_type: 10,
            bool_type: 5,
            ..Default::default()
        },
        max_depth: 1,
        ..Default::default()
    };

    let mut seen_err = false;
    let mut seen_ok = false;

    for seed in 0..200 {
        let expr_full = gen_expr_full_seeded(&db, seed, config.clone());

        fn check_for_err(db: &dyn salsa::Database, expr: &Expr, seen: &mut bool) {
            match expr {
                Expr::Error(_) => {
                    *seen = true;
                }
                Expr::List(list) => {
                    for elem in list.elements(db) {
                        check_for_err(db, &elem.expr(db).expr(db), seen);
                    }
                }
                Expr::Map(map) => {
                    for entry in map.entries(db) {
                        check_for_err(db, &entry.key(db).expr(db).expr(db), seen);
                        check_for_err(db, &entry.value(db).expr(db).expr(db), seen);
                    }
                }
                Expr::Set(set) => {
                    for elem in set.elements(db) {
                        check_for_err(db, &elem.expr(db).expr(db), seen);
                    }
                }
                Expr::AnonTuple(tuple) => {
                    for elem in tuple.elements(db) {
                        check_for_err(db, &elem.expr(db).expr(db), seen);
                    }
                }
                Expr::AnonStruct(st) => {
                    for field in st.fields(db) {
                        check_for_err(db, &field.value(db).expr(db).expr(db), seen);
                    }
                }
                Expr::Data(d) => {
                    check_for_err(db, &d.value(db).expr(db).expr(db), seen);
                }
                _ => {}
            }
        }

        if matches!(expr_full.expr(&db).expr(&db), Expr::Error(_)) {
            seen_err = true;
        } else {
            seen_ok = true;
        }

        check_for_err(&db, &expr_full.expr(&db).expr(&db), &mut seen_err);
    }

    assert!(seen_err, "Should generate at least one Result error case (Expr::Error)");
    assert!(seen_ok, "Should generate at least one Result success case (non-Err)");
}
