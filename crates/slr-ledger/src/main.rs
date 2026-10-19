//! `slr-ledger` binary: `scan` | `collect` | `regen`.
//!
//! - `scan`: print every `/// exe:` annotation as `file:line symbol item`.
//! - `collect`: run the corpus suites and write `re/out/test-results.json`.
//! - `regen` (default): rewrite the ledger section of `docs/fidelity.md`.

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let mut d = std::env::current_dir().expect("cwd");
    loop {
        if d.join("Cargo.toml").is_file() && d.join("crates").is_dir() {
            return d;
        }
        if !d.pop() {
            panic!("no workspace root above the current directory");
        }
    }
}

fn main() {
    let arg = std::env::args().nth(1);
    let root = workspace_root();
    match arg.as_deref() {
        Some("scan") => {
            for a in slr_ledger::scan_workspace(&root) {
                println!("{}:{} {} {}", a.file, a.line, a.symbol, a.item);
            }
        }
        Some("collect") => collect(&root),
        None | Some("regen") => regen(&root),
        Some(other) => {
            eprintln!("usage: slr-ledger [scan|collect|regen]");
            std::process::exit(if other == "--help" { 0 } else { 2 });
        }
    }
}

fn regen(root: &std::path::Path) {
    let annotations = slr_ledger::scan_workspace(root);
    let results_path = root.join(slr_ledger::RESULTS_PATH);
    let results = slr_ledger::read_results(&results_path).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });
    let rows = slr_ledger::build_rows(&annotations, &results);
    let section = slr_ledger::render_section(&rows);
    let doc_path = root.join(slr_ledger::FIDELITY_PATH);
    let doc = std::fs::read_to_string(&doc_path).unwrap_or_else(|e| {
        eprintln!("error: {}: {e}", doc_path.display());
        std::process::exit(1);
    });
    let out = slr_ledger::splice_section(&doc, &section);
    std::fs::write(&doc_path, &out).unwrap_or_else(|e| {
        eprintln!("error: {}: {e}", doc_path.display());
        std::process::exit(1);
    });
    println!(
        "ledger: {} symbols, {} evidence rows -> {}",
        rows.len(),
        results.iter().filter(|r| r.status == "pass").count(),
        doc_path.display()
    );
}

/// Printed by an oracle test that found its data absent and returned without
/// comparing anything. `collect` refuses to record evidence from such a test.
const ORACLE_SKIPPED_MARKER: &str = "SLR_ORACLE_SKIPPED:";

/// Corpus suites whose tests feed the ledger, with the exe symbols they
/// cover. Each entry is `(package, test_target, [(test_name, exe_symbols)])`.
/// One passing test row is recorded per symbol, so a corpus test that asserts
/// several chunks at once (e.g. methods + fields + class record) evidences
/// each of its exe symbols. An empty symbol list records the passing test
/// with no exe counterpart (kept for provenance, ignored by the ledger join).
/// One corpus test plus every exe symbol it evidences (empty = no exe counterpart).
type SuiteTest = (&'static str, Vec<&'static str>);
/// One suite: package, test target, and its tests.
type Suite = (&'static str, &'static str, Vec<SuiteTest>);

fn collect_suites() -> Vec<Suite> {
    vec![
        (
            "slr-formats",
            "corpus",
            vec![
                ("stock_install_parses_cleanly", vec![]),
                ("modded_install_parses_cleanly", vec![]),
                (
                    "stock_tufa_trees_decode_exactly",
                    vec!["FUN_0041a5f0", "FUN_0041a5c0"],
                ),
                (
                    "modded_tufa_trees_decode_exactly",
                    vec!["FUN_0041a5f0", "FUN_0041a5c0"],
                ),
                (
                    "stock_tufa_constant_pool_parses_exactly",
                    vec!["FUN_00407660"],
                ),
                (
                    "modded_tufa_constant_pool_parses_exactly",
                    vec!["FUN_00407660"],
                ),
                (
                    "stock_tufa_method_field_and_class_chunk_parses_exactly",
                    vec!["FUN_004195b0", "FUN_00419b60", "FUN_00411b20"],
                ),
                (
                    "modded_tufa_method_field_and_class_chunk_parses_exactly",
                    vec!["FUN_004195b0", "FUN_00419b60", "FUN_00411b20"],
                ),
                ("cars_rpk_tree_is_sane", vec![]),
                ("engine_block_class_names_its_base", vec![]),
                ("all_24_racer_vehicles_decode", vec!["0x00443f90"]),
                (
                    "baiern_coupesport_matches_the_cfg_numbers",
                    vec!["0x00443f90"],
                ),
            ],
        ),
        (
            "slr-fmt-cfg",
            "particle_oracle",
            vec![
                (
                    "particle_scripts_match_unicorn_vectors",
                    vec![
                        "FUN_0051ac20",
                        "FUN_00519110",
                        "FUN_0051a140",
                        "FUN_005195e0",
                        "FUN_005511c0",
                        "FUN_004369b0",
                        "FUN_0045dc80",
                        "FUN_0048f3f0",
                        "FUN_004944a0",
                        "FUN_00554c70",
                        "FUN_00553890",
                        "FUN_00553ce0",
                    ],
                ),
                (
                    "skid_scripts_match_unicorn_vectors",
                    vec!["FUN_00521e10", "FUN_00554c70", "FUN_00553890"],
                ),
            ],
        ),
        (
            "slr-fmt-skeleton",
            "oracle",
            vec![
                (
                    "skeleton_reader_matches_oracle",
                    vec!["FUN_00531e90", "FUN_00530cd0", "FUN_00553890"],
                ),
                ("animation_reader_matches_oracle", vec!["FUN_00532e70"]),
            ],
        ),
        (
            "slr-fmt-texture",
            "ptx_oracle",
            vec![(
                "ptx_loader_matches_oracle_vectors",
                vec!["FUN_00507110", "FUN_00506ad0", "FUN_00506c70"],
            )],
        ),
        (
            "slr-platform",
            "rand_oracle",
            vec![(
                "rand_matches_oracle_vectors",
                vec!["FUN_005d73fb", "FUN_005d7408", "FUN_005dd8d3"],
            )],
        ),
        (
            "slr-vm",
            "vm_oracle",
            vec![(
                "operator_helpers_match_oracle_vectors",
                vec![
                    "FUN_0041b860",
                    "FUN_0041b4d0",
                    "FUN_0041b760",
                    "FUN_0041b970",
                    "FUN_0041bbf0",
                    "FUN_0041bfb0",
                    "FUN_0041bea0",
                    "FUN_0041c290",
                    "FUN_0041c380",
                    "FUN_0041c1a0",
                    "FUN_0041aff0",
                    "FUN_0041b0e0",
                    "FUN_0041c5c0",
                    "FUN_0041c6c0",
                    "FUN_0041c860",
                    "FUN_0041cb60",
                    "FUN_0041c790",
                    "FUN_0041c9e0",
                    "FUN_0041c0c0",
                    "FUN_0041bdb0",
                    "FUN_0041b1d0",
                    "FUN_0041b2d0",
                    "FUN_0041b3d0",
                    "FUN_0041acb0",
                    "FUN_0041ab60",
                ],
            )],
        ),
        (
            "slr-vm",
            "vmstr_oracle",
            vec![(
                "string_helpers_match_oracle_vectors",
                vec!["FUN_0041b4d0", "FUN_00417a70", "FUN_00417da0"],
            )],
        ),
        (
            "slr-vm",
            "thread_oracle",
            vec![(
                "run_loop_matches_oracle_vectors",
                vec!["FUN_00420ff0", "FUN_00407d10", "FUN_00407ba0"],
            )],
        ),
        (
            "slr-vm",
            "field_oracle",
            vec![
                (
                    "field_lvalues_match_original_interpreter",
                    vec!["FUN_00420ff0"],
                ),
                (
                    "array_lvalues_match_original_interpreter",
                    vec!["FUN_00420ff0"],
                ),
                (
                    "reference_field_stores_match_original_conversion_and_assignment",
                    vec!["FUN_00417da0", "FUN_0041ab60"],
                ),
                (
                    "indexed_static_keeps_the_encoded_reference_class_like_original",
                    vec!["FUN_00420ff0"],
                ),
                (
                    "typed_declarations_match_original_interpreter",
                    vec!["FUN_00420ff0"],
                ),
                (
                    "inherited_instance_storage_matches_original_constructor_layout",
                    vec!["FUN_00420ff0"],
                ),
            ],
        ),
        // Bounded original class/instance initialization drivers. Explicit
        // metadata/allocation/log/fatal seams are in vminit_vectors.py. These
        // cases do not credit full construction, String GC or machine unwind.
        ("slr-vm", "initializers", vec![
            ("instance_initializers_observe_only_the_installed_field_prefix", vec!["FUN_00407da0"]),
            ("static_initializers_keep_partial_cells_visible_without_inventing_later_fields", vec!["FUN_00403e80"]),
            ("canonical_self_static_installs_zero_and_skips_its_initializer", vec!["FUN_00403e80"]),
            ("declared_interface_order_and_shared_dependencies_initialize_once", vec!["FUN_00403e80"]),
            ("resource_initializers_use_the_real_literal_boundary_in_both_drivers", vec!["FUN_00403e80", "FUN_00407da0"]),
            ("class_and_instance_initializers_follow_original_dependency_and_field_order", vec!["FUN_00403e80", "FUN_00407da0"]),
            ("string_initializers_store_real_interned_heap_objects_in_both_drivers", vec!["FUN_00403e80", "FUN_00407da0"]),
            ("cast_initializers_use_the_declared_float_cell_in_both_drivers", vec!["FUN_00403e80", "FUN_00407da0"]),
            ("new_initializers_allocate_real_target_objects_in_both_drivers", vec!["FUN_00403e80", "FUN_00407da0"]),
            ("script_call_initializers_return_into_the_declared_field_in_both_drivers", vec!["FUN_00403e80", "FUN_00407da0"]),
            ("dependency_kind_diagnostics_continue_initialization_like_original", vec!["FUN_00403e80", "FUN_00407da0"]),
            ("numeric_reference_initializer_conversion_preserves_original_continue_and_copy", vec!["FUN_00403e80", "FUN_00407da0"]),
            ("invalid_reference_results_overwrite_prior_writes_and_continue_like_original", vec!["FUN_00403e80", "FUN_00407da0"]),
            ("fatal_initializer_stops_before_assignment_and_retains_current_side_effect", vec!["FUN_00403e80", "FUN_00407da0"]),
            ("self_descriptor_skip_is_static_and_matches_only_the_defining_owner", vec!["FUN_00403e80", "FUN_00407da0"]),
            ("fatal_initializer_retains_prior_cell_writes_like_original", vec!["FUN_00403e80", "FUN_00407da0"]),
        ]),
        // Bounded execution observations. Recording/allocation seams are
        // documented by vmexec_vectors.py; no full call resolver, object
        // constructor, type graph, or String-GC helper is credited here.
        ("slr-vm", "execution", vec![
            ("a_non_string_pool_literal_returns_the_original_typed_null", vec!["FUN_00420ff0"]),
            ("non_member_resource_literal_returns_integer_zero_like_original", vec!["FUN_00420ff0"]),
            ("resource_members_route_package_and_raw_id_to_the_resource_world_like_original", vec!["FUN_00420ff0"]),
            ("script_frames_preserve_defining_owner_runtime_receiver_and_reverse_arguments_like_original", vec!["FUN_00420ff0"]),
            ("inline_method_references_execute_inherited_script_bodies_like_original", vec!["FUN_00420ff0"]),
            ("native_method_calls_preserve_defining_receiver_argument_order_and_return_cells_like_original", vec![]),
            ("dynamic_method_names_choose_the_same_overload_as_the_original_resolver", vec![]),
            ("dynamic_overloads_use_runtime_classes_and_preserve_null_argument_types_like_original", vec![]),
            ("new_expressions_allocate_the_requested_loaded_class_like_original", vec!["FUN_00420ff0"]),
            ("constructor_opcodes_route_runtime_this_and_super_receivers_like_original", vec!["FUN_00420ff0"]),
            ("named_instance_and_super_method_opcodes_route_and_reject_null_like_original", vec!["FUN_00420ff0"]),
            ("new_arrays_create_typed_persistent_element_cells_like_original", vec!["FUN_00420ff0"]),
            ("casts_and_instanceof_match_original_runtime_types_nulls_and_string_bytes", vec!["FUN_00420ff0"]),
            ("argument_arrays_preserve_target_lvalues_item_order_and_typed_copies_like_original", vec!["FUN_00420ff0"]),
        ]),
        ("slr-lang", "string_identity", vec![
            ("loaded_stock_string_identity_matches_real_intern_lookup_and_ptr_storage", vec![]),
            ("parsed_raw_string_literals_preserve_original_bytes_and_nul_termination", vec!["FUN_00420ff0"]),
            // No full4174a0 GC/identity credit.
        ]),
        (
            "slr-vm",
            "fields",
            vec![
                (
                    "unchanged_stock_vector_mul_matches_original_interpreter",
                    vec!["FUN_00420ff0"],
                ),
                (
                    "unchanged_stock_vector_add_matches_original_field_chains",
                    vec!["FUN_00420ff0", "FUN_00420d20"],
                ),
            ],
        ),
        (
            "slr-vm",
            "sched_oracle",
            vec![(
                "scheduler_matches_oracle_vectors",
                vec![
                    "FUN_00416940",
                    "FUN_00418c20",
                    "FUN_00418d10",
                    // The finalizer queues the driver drains and splices,
                    // appended by push_queued/push_spare/push_held.
                    "FUN_00418290",
                    "FUN_00408720",
                    "FUN_00408780",
                ],
            )],
        ),
        (
            "slr-vm",
            "value_oracle",
            vec![(
                "values_match_oracle_vectors",
                vec![
                    "FUN_00423c00",
                    "FUN_00423e90",
                    "FUN_00423e30",
                    "FUN_00423ba0",
                    "FUN_00423d40",
                    "FUN_0041f7d0",
                    "FUN_00417570",
                    "FUN_00417840",
                    "FUN_00417da0",
                    "0x515610",
                    "0x405030",
                ],
            )],
        ),
        (
            "slr-vm",
            "vmclass_oracle",
            vec![
                (
                    "interner_matches_oracle_vectors",
                    vec!["FUN_0041e4d0", "FUN_0041e610", "FUN_0041ef10"],
                ),
                (
                    "registrar_matches_oracle_vectors",
                    vec!["FUN_00416b00", "FUN_0041a370"],
                ),
                ("boot_binds_match_oracle_vectors", vec!["FUN_00423e10"]),
            ],
        ),
        (
            "slr-vm",
            "corpus",
            vec![
                (
                    "stock_class_files_load_and_link_in_isolated_namespaces",
                    vec!["FUN_0040fe70", "FUN_00411b20", "FUN_00410430"],
                ),
                ("native_table_reports_353_bindings", vec!["FUN_00416b00"]),
            ],
        ),
        (
            "slr-io",
            "io_oracle",
            vec![
                ("the_path_rows_replay", vec!["LAB_00486150", "LAB_00486170"]),
                (
                    "the_copy_rows_replay",
                    vec!["FUN_004860f0", "FUN_0054c550", "FUN_00558a50"],
                ),
                (
                    "the_move_rows_replay",
                    vec!["FUN_00486120", "FUN_0054c650", "FUN_00558ba0"],
                ),
                ("the_open_rows_replay", vec!["LAB_00485440"]),
                ("the_close_rows_replay", vec!["LAB_00485870"]),
                (
                    "the_findfile_rows_replay",
                    vec!["LAB_00487cb0", "LAB_00487da0", "FUN_00487e70"],
                ),
                ("the_write_resource_ref_rows_replay", vec!["FUN_00485c60"]),
                (
                    "the_string_rows_replay",
                    vec!["LAB_00485fd0", "LAB_00486060"],
                ),
                ("the_read_res_id_rows_replay", vec!["FUN_00485ea0"]),
                (
                    "the_input_stub_rows_replay",
                    vec![
                        "FUN_0047c990",
                        "FUN_0047c9c0",
                        "LAB_0047c9f0",
                        "LAB_0047ca70",
                        "LAB_0047cae0",
                        "LAB_0047cb00",
                        "LAB_0047cc10",
                        "LAB_0047cc50",
                        "LAB_0047cd00",
                        "FUN_0047cd40",
                        "LAB_0047ce80",
                        "LAB_00487870",
                        "LAB_00487970",
                        "0x477740",
                        "0x477810",
                        "0x4778b0",
                        "0x477970",
                        "0x477a00",
                        "0x477a80",
                        "0x477b20",
                    ],
                ),
            ],
        ),
        (
            "slr-render-natives",
            "render_oracle",
            vec![(
                "the_render_rows_replay",
                vec![
                    "0x47c1d0", "0x47c2d0", "0x47c390", "0x47c3f0", "0x47c440", "0x47c450",
                    "0x47c460", "0x481680", "0x4816c0", "0x481700", "0x4817b0", "0x4817f0",
                    "0x481870", "0x4818f0", "0x481970", "0x487020", "0x487040",
                ],
            )],
        ),
        (
            "slr-sound",
            "sound_oracle",
            vec![(
                "the_sound_rows_replay",
                vec![
                    "0x487460", "0x487500", "0x487510", "0x487520", "0x487560", "0x487580",
                    "0x4875f0", "0x487640", "0x487650", "0x487f00",
                ],
            )],
        ),
        (
            "slr-io",
            "filelayer_oracle",
            vec![(
                "the_file_layer_rows_replay",
                vec![
                    "0x54c880", "0x54c8d0", "0x54cca0", "0x54cdd0", "0x54ce40", "0x54cec0",
                    "0x54cf00", "0x554a10",
                ],
            )],
        ),
        (
            "slr-fmt-sdat",
            "sdat_oracle",
            vec![
                (
                    "file_natives_match_oracle_vectors",
                    vec![
                        "FUN_00485b20",
                        "FUN_00485bc0",
                        "FUN_00485b70",
                        "FUN_00485c10",
                    ],
                ),
                (
                    "slot_run_store_matches_oracle_vectors",
                    vec!["FUN_00484fa0"],
                ),
                ("file_close_matches_oracle_vectors", vec!["0x00485870"]),
            ],
        ),
        (
            "slr-fmt-scx",
            "corpus",
            vec![
                ("stock_scx_v3_and_scy_decode_exactly", vec!["FUN_004b5c30"]),
                ("modded_scx_v3_and_scy_decode_exactly", vec!["FUN_004b5c30"]),
            ],
        ),
        (
            "slr-resource",
            "resource_oracle",
            vec![(
                "resource_native_effects_match_oracle_vectors",
                vec![
                    "LAB_0047cf00",
                    "LAB_0047cea0",
                    "FUN_0047cf80",
                    "FUN_0047cfc0",
                    "LAB_0047d0a0",
                    "LAB_0047d170",
                    "LAB_0047d1e0",
                    "LAB_0047d210",
                    "LAB_0047d290",
                    "LAB_0047d3e0",
                    "LAB_0047d4b0",
                    "LAB_0047d570",
                    "LAB_0047d630",
                    "LAB_0047d6f0",
                    "LAB_0047ffe0",
                    "LAB_00480100",
                    "LAB_0047e8d0",
                    "LAB_0047e950",
                    "FUN_004802c0",
                    "LAB_00480390",
                    "LAB_0047df40",
                    "FUN_00486c80",
                    "FUN_00486cd0",
                    "LAB_0047df80",
                ],
            )],
        ),
        (
            "slr-resource",
            "game_oracle",
            vec![(
                "game_query_natives_match_oracle_vectors",
                vec![
                    "LAB_0047d7b0",
                    "LAB_0047d900",
                    "LAB_0047dad0",
                    "LAB_0047dbe0",
                    "LAB_0047dce0",
                    "LAB_0047ddf0",
                    "LAB_0047da30",
                    "FUN_0047e2d0",
                    "FUN_0047e350",
                    "FUN_0047e490",
                    "FUN_0047e630",
                    "LAB_00486d10",
                    "LAB_00486da0",
                    "LAB_00486f30",
                    "LAB_00480440",
                ],
            )],
        ),
        (
            "slr-resource",
            "wrapper_oracle",
            vec![
                (
                    "resource_wrappers_are_real_typed_vm_objects",
                    vec!["FUN_0047d300"],
                ),
                (
                    "stock_interpreter_ptr_write_is_observed_by_installed_resource_natives",
                    vec!["FUN_00420ff0"],
                ),
            ],
        ),
        (
            "slr-resource",
            "res_oracle",
            vec![
                (
                    "anim_queue_matches_oracle_vectors",
                    vec![
                        "LAB_0047ec00",
                        "LAB_0047ecc0",
                        "LAB_0047ed80",
                        "LAB_0047ee30",
                        "LAB_0047eee0",
                        "LAB_0047ef90",
                    ],
                ),
                (
                    "particle_actions_match_oracle_vectors",
                    vec![
                        "FUN_0047f150",
                        "FUN_0047f1f0",
                        "FUN_0047fcd0",
                        "FUN_0047fdb0",
                        "LAB_0047fa70",
                        "LAB_0047f270",
                        "LAB_0047f660",
                    ],
                ),
                (
                    "handle_gates_match_oracle_vectors",
                    vec![
                        "FUN_00480b00",
                        "LAB_00480c10",
                        "LAB_00480920",
                        "FUN_00480d40",
                        "FUN_00480e60",
                        "LAB_00480440",
                        "LAB_00486be0",
                    ],
                ),
            ],
        ),
        // Every cfg keyword handler the stock+modded corpus actually exercises
        // (`eof` has no handler; the other 54 table keywords never appear in the
        // corpus, so they get no evidence row). See `docs/physics.md` (`#50`).
        (
            "slr-physics",
            "cfg_handlers_oracle",
            vec![(
                "cfg_handlers_match_unicorn_vectors",
                vec![
                    "FUN_00442f90",
                    "FUN_004431c0",
                    "FUN_00443310",
                    "FUN_004433e0",
                    "FUN_004438d0",
                    "FUN_00443a00",
                    "FUN_00443a60",
                    "FUN_00443b00",
                    "FUN_00443d40",
                    "FUN_00443de0",
                    "FUN_00443ea0",
                    "FUN_00443f90",
                    "FUN_00444080",
                    "FUN_00444150",
                    "FUN_00444190",
                    "FUN_00444260",
                    "FUN_004443c0",
                    "FUN_004445f0",
                    "FUN_00444730",
                    "FUN_00444740",
                    "FUN_00444810",
                    "FUN_00444990",
                    "FUN_00444b60",
                    "FUN_00444cb0",
                    "FUN_00445c80",
                    "FUN_00445e00",
                    "FUN_00445ef0",
                    "FUN_004460a0",
                    "FUN_00446160",
                    "FUN_0046b950",
                    "FUN_0046bb80",
                    "FUN_0046bfa0",
                    "FUN_0046c280",
                    "FUN_0046c2d0",
                    "FUN_0046c310",
                    "FUN_0046c360",
                    "FUN_0046c3a0",
                    "FUN_0046c6a0",
                    "FUN_0046c770",
                    "FUN_0046c840",
                    "FUN_0046c8a0",
                    "FUN_0046c8f0",
                    "FUN_0046cb30",
                    "FUN_0046cdf0",
                    "FUN_0046ce60",
                    "FUN_0046ced0",
                    "FUN_0046cf50",
                    "FUN_0046d0c0",
                    "FUN_0046d180",
                    "FUN_0046d290",
                    "FUN_0046d330",
                    "FUN_0046d8a0",
                    "FUN_0046d970",
                ],
            )],
        ),
        (
            "slr-lang",
            "langobj_oracle",
            vec![(
                "object_natives_match_oracle_vectors",
                vec![
                    "LAB_0047c8c0",
                    "LAB_0047c8e0",
                    "FUN_00408720",
                    "FUN_00408780",
                    "LAB_0047be50",
                    "LAB_0047be60",
                    "LAB_0047be80",
                    "FUN_0047bed0",
                    "LAB_0047bef0",
                    "LAB_0047bf10",
                    "LAB_0047c060",
                    "LAB_0047c070",
                    "LAB_0047c0c0",
                    "LAB_0047c870",
                    "LAB_0047c900",
                    "FUN_0047c920",
                    "FUN_0047bf80",
                    "LAB_0047bf30",
                    "LAB_0047bff0",
                    "FUN_0047c4b0",
                    "LAB_0047c3b0",
                    "LAB_0047bfd0",
                    "LAB_0047bfe0",
                    "LAB_0047c0b0",
                    "LAB_0047c950",
                    "LAB_0047c970",
                    "FUN_0047c6e0",
                    "FUN_0047c790",
                    "LAB_0047c730",
                    "LAB_0047c760",
                    "FUN_00481ed0",
                    "FUN_00481f10",
                    "LAB_00481fe0",
                    "LAB_00481f50",
                    "LAB_00486190",
                    "LAB_00481cf0",
                ],
            )],
        ),
        (
            "slr-lang",
            "engine_oracle",
            vec![(
                "engine_natives_match_oracle",
                vec![
                    "LAB_0047c510",
                    "LAB_0047c570",
                    "LAB_0047c600",
                    "FUN_0047c080",
                    "LAB_0047c500",
                    "LAB_0047c3a0",
                    "LAB_00487be0",
                    "LAB_00487660",
                    "LAB_0047e000",
                    "LAB_0047e050",
                    "LAB_0047e100",
                    "LAB_0047e1c0",
                    "LAB_0047e260",
                    "FUN_004819f0",
                    "FUN_00481a30",
                    "FUN_00481c40",
                    "FUN_00481c80",
                    "LAB_00481cc0",
                    "LAB_00481a70",
                ],
            )],
        ),
        (
            "slr-lang",
            "clock_init_oracle",
            vec![(
                "initialized_counter_sequences_match_original",
                vec!["FUN_00551690", "FUN_005516c0"],
            )],
        ),
        (
            "slr-lang",
            "clock_oracle",
            vec![(
                "counter_clock_matches_original_queries_and_x87_return",
                vec!["FUN_005516c0"],
            )],
        ),
        (
            "slr-lang",
            "langmath_oracle",
            vec![(
                "math_natives_match_oracle_vectors",
                vec![
                    "LAB_0047c7d0",
                    "FUN_0047c7f0",
                    "LAB_0047c820",
                    "LAB_0047c850",
                    "LAB_00481dd0",
                    "LAB_00481e30",
                    "LAB_00481e80",
                    "FUN_00553d70",
                    "FUN_00554170",
                    "FUN_0045d910",
                ],
            )],
        ),
        (
            "slr-lang",
            "vector_oracle",
            vec![(
                "vector_natives_match_oracle_vectors",
                vec![
                    "LAB_00482110",
                    "LAB_004823e0",
                    "LAB_00482550",
                    "LAB_00482450",
                    "LAB_00482280",
                    "LAB_00482020",
                    "LAB_00482610",
                    "LAB_004826c0",
                    "FUN_0054ecd0",
                    "FUN_0054eab0",
                    "FUN_0054ee80",
                    "FUN_00551c90",
                    "FUN_00551d20",
                ],
            )],
        ),
    ]
}

/// Run the corpus suites, parse the human-readable `cargo test` output
/// (stable across libtest versions, unlike `--format json`), and write
/// `re/out/test-results.json`.
fn collect(root: &std::path::Path) {
    let today = today_iso();
    // A symbol spelled differently from its `/// exe:` annotation records
    // evidence nothing can join with, and the row silently keeps reading
    // `ported (no test yet)`. Fail instead: every registered symbol must be
    // one the scanner actually emits.
    let annotated: std::collections::HashSet<String> = slr_ledger::scan_workspace(root)
        .into_iter()
        .map(|a| a.symbol)
        .collect();
    let registry = native_registry(root);
    for (package, target, tests) in collect_suites() {
        for (name, symbols) in &tests {
            for symbol in symbols {
                if !annotated.contains(*symbol) {
                    eprintln!(
                        "error: {package}::{target}::{name} registers {symbol}, \
                         which no `/// exe:` annotation uses"
                    );
                    std::process::exit(1);
                }
                // The registry is the exe's own label for that address
                // (`re/out/native_api.tsv`, built from the registrar call
                // sites). An annotation that says FUN_ where the exe says
                // LAB_ joins to nothing downstream, so it is a misspelling
                // even though it scans.
                if let Some(want) = registry.get(&symbol_address(symbol)) {
                    if *want != *symbol {
                        eprintln!(
                            "error: {package}::{target}::{name} registers {symbol}, \
                             but the exe labels that address {want}"
                        );
                        std::process::exit(1);
                    }
                }
            }
        }
    }
    let stock = std::env::var("SLR_STOCK")
        .unwrap_or_else(|_| "/Users/nicolas/Documents/github/games/street-legal/stock".to_string());
    let reference = std::env::var("SLR_REFERENCE").unwrap_or_else(|_| {
        "/Users/nicolas/Documents/github/games/street-legal/reference/raw".to_string()
    });
    let mut results: Vec<slr_ledger::TestResult> = Vec::new();
    for (package, target, tests) in collect_suites() {
        let output = std::process::Command::new("cargo")
            // `--nocapture`: a skipped oracle prints its note and returns, and
            // libtest hides captured output for a passing test, so without this
            // the guard below cannot see the skip.
            .args(["test", "-p", package, "--test", target, "--", "--nocapture"])
            .env("SLR_STOCK", &stock)
            .env("SLR_REFERENCE", &reference)
            .current_dir(root)
            .output()
            .unwrap_or_else(|e| {
                eprintln!("error: cannot run cargo test: {e}");
                std::process::exit(1);
            });
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}\n{stderr}");
        // A test that finds its data absent prints `SKIP ...` and returns, which
        // libtest reports as `ok`: it would vouch for natives while comparing
        // nothing. `SLR_ORACLE_SKIPPED: <test>` is the unambiguous marker for
        // that (a row-level `SKIP` inside a suite that did run is not a whole
        // test skipping), so a whole-test skip fails collect outright.
        if let Some(test) = skipped_test(&combined) {
            eprintln!(
                "error: {package} --test {target} skipped {test}, which compared nothing, \
                 so it proves nothing"
            );
            std::process::exit(1);
        }
        let statuses = parse_libtest_statuses(&combined);
        if !output.status.success() {
            eprintln!("error: cargo test -p {package} --test {target} failed:\n{combined}");
            std::process::exit(1);
        }
        for (name, exe_symbols) in &tests {
            let status = statuses.get(*name).cloned().unwrap_or_else(|| {
                eprintln!("error: test {name} did not run in {package} --test {target}");
                std::process::exit(1);
            });
            // Suites whose target consumes Unicorn oracle vectors (rather
            // than the stock corpus) record the `unicorn` oracle.
            let oracle = if matches!(
                target,
                "rand_oracle"
                    | "value_oracle"
                    | "sdat_oracle"
                    | "oracle"
                    | "particle_oracle"
                    | "vm_oracle"
                    | "thread_oracle"
                    | "field_oracle"
                    | "execution"
                    | "initializers"
                    | "string_identity"
                    | "fields"
                    | "sched_oracle"
                    | "vmclass_oracle"
                    | "ptx_oracle"
                    | "vmstr_oracle"
                    | "res_oracle"
                    | "cfg_handlers_oracle"
                    | "vector_oracle"
                    | "langmath_oracle"
                    | "clock_oracle"
                    | "clock_init_oracle"
                    | "langobj_oracle"
                    | "engine_oracle"
                    | "io_oracle"
                    | "render_oracle"
                    | "sound_oracle"
                    | "resnode_oracle"
                    | "resource_oracle"
                    | "game_oracle"
                    | "wrapper_oracle"
                    | "filelayer_oracle"
                    | "resref_oracle"
                    | "gametype_oracle"
            ) {
                "unicorn"
            } else {
                "corpus"
            };
            if exe_symbols.is_empty() {
                results.push(slr_ledger::TestResult {
                    test: name.to_string(),
                    krate: package.to_string(),
                    exe_symbol: None,
                    oracle: oracle.to_string(),
                    status: status.clone(),
                    ran_on: today.clone(),
                });
            }
            for sym in exe_symbols {
                results.push(slr_ledger::TestResult {
                    test: name.to_string(),
                    krate: package.to_string(),
                    exe_symbol: Some(sym.to_string()),
                    oracle: oracle.to_string(),
                    status: status.clone(),
                    ran_on: today.clone(),
                });
            }
        }
    }
    let path = root.join(slr_ledger::RESULTS_PATH);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| {
            eprintln!("error: {}: {e}", parent.display());
            std::process::exit(1);
        });
    }
    let json = serde_json::to_string_pretty(&results).expect("serialize results");
    std::fs::write(&path, format!("{json}\n")).unwrap_or_else(|e| {
        eprintln!("error: {}: {e}", path.display());
        std::process::exit(1);
    });
    let passed = results.iter().filter(|r| r.status == "pass").count();
    println!(
        "collect: {passed}/{} pass -> {}",
        results.len(),
        path.display()
    );
}

/// The address a `FUN_`/`LAB_` symbol names, or 0 for anything else (an
/// unnamed `0x...` row, a helper without a registry entry).
fn symbol_address(symbol: &str) -> u32 {
    symbol
        .strip_prefix("FUN_")
        .or_else(|| symbol.strip_prefix("LAB_"))
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .unwrap_or(0)
}

/// Every registered native's exe label by address, so a registration can be
/// checked against the label the exe actually uses. Keyed by address, not by
/// spelling: a wrong `FUN_`/`LAB_` prefix still finds the row. Rows are
/// `class<TAB>name<TAB>sig<TAB>impl`; an absent file yields an empty map and
/// the label check is skipped.
fn native_registry(root: &std::path::Path) -> std::collections::BTreeMap<u32, &'static str> {
    let path = root.join("re/out/native_api.tsv");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return std::collections::BTreeMap::new();
    };
    // Leaked once: the table lives for the whole process and the map borrows it.
    let text: &'static str = Box::leak(text.into_boxed_str());
    text.lines()
        .filter_map(|line| line.rsplit('\t').next())
        .map(|impl_label| (symbol_address(impl_label), impl_label))
        .collect()
}

/// The test named by a whole-test skip marker in `output`, if any. An oracle
/// test that finds its vectors absent prints `SLR_ORACLE_SKIPPED: <test>` and
/// returns; libtest still reports it `ok`, so `collect` must refuse its rows.
fn skipped_test(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (_, rest) = line.split_once(ORACLE_SKIPPED_MARKER)?;
        let name = rest.trim();
        Some(
            name.strip_prefix('<')
                .and_then(|n| n.strip_suffix('>'))
                .unwrap_or(name)
                .to_string(),
        )
    })
}

/// Parse `test <name> ... ok|FAILED|ignored` lines from `cargo test` output.
fn parse_libtest_statuses(output: &str) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    for line in output.lines() {
        let line = line.trim_start_matches(|c: char| c.is_whitespace());
        // libtest prints `test <name> ... ok`; strip any leading
        // `test ` after harness prefixes like `running` lines are skipped.
        let Some(rest) = line.strip_prefix("test ") else {
            continue;
        };
        let Some((name, tail)) = rest.split_once(" ... ") else {
            continue;
        };
        let status = if tail.starts_with("ok") {
            "pass"
        } else if tail.starts_with("FAILED") {
            "fail"
        } else {
            continue; // `ignored`, etc.
        };
        map.insert(name.trim().to_string(), status.to_string());
    }
    map
}

/// Today's date as `YYYY-MM-DD`, via the `date` command (no extra deps).
fn today_iso() -> String {
    std::process::Command::new("date")
        .arg("+%F")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| s.len() == 10)
        .unwrap_or_else(|| "1970-01-01".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_test_skip_is_named() {
        let out = "\nSLR_ORACLE_SKIPPED: vector_natives_match_oracle_vectors\n\
                   SKIP vector_oracle: absent\n\
                   test vector_natives_match_oracle_vectors ... ok\n";
        assert_eq!(
            skipped_test(out).as_deref(),
            Some("vector_natives_match_oracle_vectors")
        );
    }

    #[test]
    fn a_symbol_address_ignores_the_prefix() {
        // The registry lookup must not depend on FUN_/LAB_ spelling.
        assert_eq!(symbol_address("LAB_0047c950"), 0x47c950);
        assert_eq!(symbol_address("FUN_0047c950"), 0x47c950);
        assert_eq!(symbol_address("0x00485870"), 0);
        assert_eq!(symbol_address("FUN_00408110"), 0x408110);
    }

    #[test]
    fn a_row_level_skip_is_not_a_whole_test_skip() {
        // `vmstr_oracle` drops one undefined-frame row and still compares the
        // rest; that must not make collect refuse the suite.
        let out = "vmstr_oracle: vmstr_conv[5]: SKIP pointer-identity artifact\n\
                   test string_helpers_match_oracle_vectors ... ok\n";
        assert_eq!(skipped_test(out), None);
    }
}
