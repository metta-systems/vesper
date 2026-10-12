//! Tests of the Meddle parser, checks and generator.

#[cfg(test)]
mod tests {
    use {
        libmeddle::{
            LinkSummary, SourceFile,
            ast::{Interface, Link},
            check::{self, Checked},
            emit, parse,
        },
        std::path::PathBuf,
    };

    const MAILBOX: &str = include_str!("../../../drivers/mailbox/interfaces/mailbox_v1.if");

    fn source(name: &str, text: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from(name),
            source: text.to_owned(),
        }
    }

    /// The only interface of `text`, which must parse cleanly.
    fn interface(text: &str) -> Interface {
        let (file, diagnostics) = parse(text);
        assert!(
            diagnostics.is_empty(),
            "unexpected parse errors: {diagnostics:?}"
        );
        let mut file = file.expect("a parsed file");
        assert_eq!(file.interfaces.len(), 1);
        file.interfaces.remove(0)
    }

    /// The messages `check` reports for the interface in `text`.
    fn check_messages(text: &str) -> Vec<String> {
        let interface = interface(text);
        match check::check(&interface) {
            Ok(_) => Vec::new(),
            Err(diagnostics) => diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect(),
        }
    }

    fn assert_reports(text: &str, expected: &str) {
        let messages = check_messages(text);
        assert!(
            messages.iter().any(|message| message.contains(expected)),
            "expected a diagnostic containing {expected:?}, got {messages:?}"
        );
    }

    fn with_checked<T>(text: &str, use_checked: impl FnOnce(&Checked<'_>) -> T) -> T {
        let interface = interface(text);
        let checked = check::check(&interface)
            .unwrap_or_else(|diagnostics| panic!("unexpected check errors: {diagnostics:?}"));
        use_checked(&checked)
    }

    fn type_code(text: &str) -> u64 {
        let interface = interface(text);
        check::check(&interface).map_or(0, |checked| checked.type_code())
    }

    #[test]
    fn mailbox_interface_parses_with_its_documentation() {
        let mailbox = interface(MAILBOX);
        assert_eq!(mailbox.name.text, "mailbox_v1");
        assert!(mailbox.local);
        assert_eq!(mailbox.types.len(), 2);
        assert_eq!(mailbox.exceptions.len(), 3);
        assert_eq!(mailbox.methods.len(), 6);
        assert_eq!(
            mailbox.doc.first().map(String::as_str),
            Some("The `VideoCore` firmware's property mailbox (Raspberry Pi), served by the")
        );
        let property = mailbox.methods.last().expect("property");
        assert_eq!(property.name.text, "property");
        assert!(property.parameters[0].ty.reference);
        assert_eq!(property.raises.len(), 3);
    }

    #[test]
    fn mailbox_interface_passes_the_checks() {
        with_checked(MAILBOX, |checked| {
            let set_power_state = &checked.interface().methods[4];
            assert_eq!(checked.words_of_fields(&set_power_state.parameters), 2);
            let property = &checked.interface().methods[5];
            assert_eq!(checked.words_of_fields(&property.parameters), 2);
        });
    }

    #[test]
    fn a_broken_member_is_reported_and_the_rest_still_parses() {
        let (file, diagnostics) = parse(
            "local interface broken_v1 {
                first(card32 a) returns (card32 b);
                second(card32 ) ;
                enum colour { red, green }
                third(card32 card32 c);
                fourth();
            }",
        );
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        let interface = &file.expect("the interface still parses").interfaces[0];
        let names: Vec<&str> = interface
            .methods
            .iter()
            .map(|method| method.name.text.as_str())
            .collect();
        assert_eq!(names, ["first", "fourth"]);
        assert_eq!(interface.types.len(), 1);
    }

    #[test]
    fn parse_errors_say_what_was_expected_and_point_at_it() {
        let text = "interface oops_v1 { method(card32 a) returns card32; }";
        let (_, diagnostics) = parse(text);
        let diagnostic = diagnostics.first().expect("a parse error");
        assert!(
            diagnostic.message.contains("expected `(`"),
            "{}",
            diagnostic.message
        );
        assert_eq!(text.get(diagnostic.span.clone()), Some("card32"));
        let rendered = diagnostic.render("oops_v1.if", text, false);
        assert!(rendered.contains("oops_v1.if:1:"), "{rendered}");
    }

    #[test]
    fn interfaces_carry_a_version() {
        assert_reports("interface mailbox { f(); }", "has no version");
        assert_reports("interface mailbox_vx { f(); }", "has no version");
        assert!(check_messages("interface mailbox_v12 { f(); }").is_empty());
    }

    #[test]
    fn names_resolve_and_are_unique() {
        assert_reports("interface a_v1 { f(colour c); }", "unknown type `colour`");
        assert_reports(
            "interface a_v1 { f(); f(); }",
            "function `f` is defined twice",
        );
        assert_reports(
            "interface a_v1 { enum e { x, x } }",
            "value `x` is defined twice",
        );
        assert_reports(
            "interface a_v1 { f() raises (gone); }",
            "no exception `gone`",
        );
        assert_reports(
            "interface a_v1 { record r { r inner; } }",
            "type `r` contains itself",
        );
    }

    #[test]
    fn names_must_work_as_rust_identifiers() {
        assert_reports(
            "interface a_v1 { f(card32 match); }",
            "`match` is a reserved word in Rust",
        );
        assert_reports(
            "interface a_v1 { record error {} }",
            "which the generated code defines",
        );
        assert_reports(
            "interface a_v1 { exception a_v1 {} }",
            "which the generated code defines",
        );
    }

    #[test]
    fn references_are_local_parameters_only() {
        assert_reports(
            "interface a_v1 { f(octet& buffer); }",
            "need a `local` interface",
        );
        assert_reports(
            "local interface a_v1 { data 4096; record r { octet& buffer; } }",
            "only on function parameters",
        );
        assert_reports(
            "local interface a_v1 { data 4096; f() returns (octet& buffer); }",
            "only on function parameters",
        );
        assert!(check_messages("local interface a_v1 { data 4096; f(octet& buffer); }").is_empty());
    }

    #[test]
    fn data_areas_go_with_references() {
        assert_reports(
            "local interface a_v1 { f(octet& buffer); }",
            "need a `data <bytes>;` area",
        );
        assert_reports(
            "local interface a_v1 { data 4096; f(card32 value); }",
            "no function takes a reference",
        );
        assert_reports(
            "local interface a_v1 { data 0; f(octet& buffer); }",
            "at least one page",
        );
        assert_reports(
            "local interface a_v1 { data 100; f(octet& buffer); }",
            "whole 4096-byte pages",
        );
        assert_reports(
            "local interface a_v1 { data 131072; f(octet& buffer); }",
            "at most 65536 bytes",
        );
        assert!(
            check_messages("local interface a_v1 { data 65536; f(octet& buffer); }").is_empty()
        );
    }

    #[test]
    fn data_area_size_is_part_of_the_type_code_and_the_generated_api() {
        let one_page = type_code("local interface a_v1 { data 4096; f(octet& buffer); }");
        let two_pages = type_code("local interface a_v1 { data 8192; f(octet& buffer); }");
        assert_ne!(one_page, two_pages);

        let (interface_module, client_module) = with_checked(
            "local interface a_v1 { data 8192; f(octet& buffer); }",
            |checked| {
                (
                    emit::interface_module(checked, "a_v1.if"),
                    emit::client_module(checked, "a_v1.if", "a"),
                )
            },
        );
        assert!(interface_module.contains("pub const DATA_BYTES: u64 = 8192;"));
        assert!(client_module.contains("pub data_area: Option<__runtime::DataArea>"));
        assert!(client_module.contains("pub const fn data_area(&self)"));
        let mailbox = libmeddle::summarize_interface(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../drivers/mailbox/interfaces/mailbox_v1.if"
        ))
        .expect("mailbox_v1 summary");
        assert_eq!(mailbox.data_bytes, 4096);
    }
    #[test]
    fn calls_fit_the_register_budget() {
        assert!(
            check_messages(
                "interface a_v1 { f(card64 a, card64 b, card64 c, card64 d, card64 e, card64 f); }"
            )
            .is_empty()
        );
        assert_reports(
            "interface a_v1 { f(card64 a, card64 b, card64 c, card64 d, card64 e, card64 f, card64 g); }",
            "takes 7 argument words",
        );
        // A reference takes two words.
        assert_reports(
            "local interface a_v1 { data 4096; f(octet& a, octet& b, octet& c, card32 d); }",
            "takes 7 argument words",
        );
        assert_reports(
            "interface a_v1 { record pair { card32 a; card32 b; } f() returns (pair p); }",
            "returns 2 words",
        );
        assert_reports(
            "interface a_v1 { f() returns (card32 a, card32 b); }",
            "returns 2 words",
        );
        assert_reports(
            "interface a_v1 { exception wide { card32 a; card32 b; } }",
            "carries 2 words",
        );
    }

    #[test]
    fn export_names_fit_the_image_record() {
        assert_reports(
            "interface a_rather_long_name_v1 { a_rather_long_function(); }",
            "at most 32 are recorded",
        );
    }

    #[test]
    fn interface_files_hold_one_interface_named_after_the_file() {
        let file = source("mailbox_v2.if", "interface mailbox_v1 { f(); }");
        let failure = file.interface().expect_err("a misnamed interface file");
        assert!(
            failure.diagnostics[0]
                .message
                .contains("must be in `mailbox_v1.if`")
        );

        let file = source("a_v1.if", "interface a_v1 { f(); } interface b_v1 { g(); }");
        let failure = file.interface().expect_err("two interfaces in one file");
        assert!(
            failure.diagnostics[0]
                .message
                .contains("exactly one interface")
        );

        let file = source(
            "a_v1.if",
            "interface a_v1 { f(); } uses b_v1 via links { g }",
        );
        let failure = file.interface().expect_err("uses in an interface file");
        assert!(
            failure.diagnostics[0]
                .message
                .contains("belongs in imports.if")
        );
    }

    #[test]
    fn imports_name_existing_functions() {
        let directories = [PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../drivers/mailbox/interfaces"
        ))];
        let imports = source(
            "imports.if",
            "uses mailbox_v1 via mboxes { get_clock_rate, set_power_state }",
        );
        let uses = imports.imports(&directories).expect("valid imports");
        assert_eq!(uses[0].functions.len(), 2);

        let imports = source(
            "imports.if",
            "uses mailbox_v1 via mboxes { get_clock_rate, reboot }",
        );
        let failure = imports
            .imports(&directories)
            .expect_err("an unknown function");
        assert_eq!(failure.diagnostics.len(), 1);
        assert!(
            failure.diagnostics[0]
                .message
                .contains("has no function `reboot`")
        );

        // A chain of links reaches the exporter through intermediate nodes.
        let (file, diagnostics) = parse(
            "uses mailbox_v1 via firmware, mboxes { get_clock_rate }\n\
             uses mailbox_v1 via \"raspberrypi,firmware\", mboxes { get_board_serial }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let file = file.expect("parsed uses");
        let links: Vec<Vec<&str>> = file
            .uses
            .iter()
            .map(|uses| {
                uses.links
                    .iter()
                    .map(|link| link.name().text.as_str())
                    .collect()
            })
            .collect();
        assert_eq!(
            links,
            [["firmware", "mboxes"], ["raspberrypi,firmware", "mboxes"]]
        );

        // A chain can start at the one node with a compatible string.
        let (file, diagnostics) = parse(
            "uses mailbox_v1 via compatible \"raspberrypi,bcm2835-firmware\", mboxes { get_board_serial }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let file = file.expect("parsed uses");
        assert!(matches!(
            file.uses[0].links.as_slice(),
            [Link::Compatible(first), Link::Property(second)]
                if first.text == "raspberrypi,bcm2835-firmware" && second.text == "mboxes"
        ));
        let directories = [PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../drivers/mailbox/interfaces"
        ))];
        let misplaced = source(
            "imports.if",
            "uses mailbox_v1 via firmware, compatible \"x\" { get_board_serial }",
        );
        let failure = misplaced
            .imports(&directories)
            .expect_err("a compatible link after the first");
        assert!(
            failure.diagnostics[0]
                .message
                .contains("can only start a chain")
        );

        // A use names the device tree property linking to the exporter.
        let (_, diagnostics) = parse("uses mailbox_v1 { get_clock_rate }");
        assert!(
            diagnostics[0].message.contains("expected `via`"),
            "{diagnostics:?}"
        );

        let imports = source("imports.if", "uses timer_v1 via timers { now }");
        let failure = imports
            .imports(&directories)
            .expect_err("an unknown interface");
        assert!(
            failure.diagnostics[0]
                .message
                .contains("no interface file `timer_v1.if`")
        );
    }

    #[test]
    fn import_summaries_name_functions_by_declaration_index() {
        let directory = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../drivers/mailbox/interfaces"
        ));
        let interfaces = libmeddle::interface_files(&directory);
        assert_eq!(
            interfaces,
            [directory.join("mailbox_v1.if")],
            "imports.if is not an interface"
        );
        let mailbox = libmeddle::summarize_interface(&interfaces[0]).expect("mailbox_v1 summary");
        assert_eq!(mailbox.functions.len(), 6);
        assert_eq!(
            mailbox.export_name(2).as_deref(),
            Some("mailbox_v1_get_clock_rate")
        );

        let imports = std::env::temp_dir().join(format!("meddle-imports-{}", std::process::id()));
        std::fs::create_dir_all(&imports).expect("a scratch directory");
        let imports_file = imports.join("imports.if");
        std::fs::write(
            &imports_file,
            "uses mailbox_v1 via \"raspberrypi,firmware\" { set_power_state, get_clock_rate }",
        )
        .expect("the imports file");
        let summaries =
            libmeddle::summarize_imports(&imports_file, &[directory]).expect("valid imports");
        std::fs::remove_dir_all(&imports).ok();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].interface.type_code, mailbox.type_code);
        // In the order `uses` names them, as declaration indices.
        assert_eq!(summaries[0].functions, [4, 2]);
        assert_eq!(
            summaries[0].links,
            [LinkSummary::Property("raspberrypi,firmware".to_owned())]
        );
    }
    #[test]
    fn type_code_ignores_documentation_and_tracks_signatures() {
        let base = type_code("interface a_v1 { f(card32 x); }");
        let documented = type_code("## Documented.\ninterface a_v1 {\n ## f.\n f(card32 x); }");
        let changed = type_code("interface a_v1 { f(card64 x); }");
        assert_eq!(base, documented);
        assert_ne!(base, changed);
        assert_eq!(base & 0xffff, 0, "the low 16 bits are left for subtypes");
    }

    #[test]
    fn generated_modules_are_valid_rust() {
        let interface_module = with_checked(MAILBOX, |checked| {
            emit::interface_module(checked, "mailbox_v1.if")
        });
        let client_module = with_checked(MAILBOX, |checked| {
            emit::client_module(checked, "mailbox_v1.if", "mailbox")
        });
        for (module, items) in [
            (
                &interface_module,
                [
                    "pub trait MailboxV1",
                    "macro_rules! mailbox_v1_export",
                    "pub enum Exception",
                ],
            ),
            (
                &client_module,
                ["pub struct Client", "pub struct Bindings", "pub enum Error"],
            ),
        ] {
            syn::parse_file(module).expect("generated code parses");
            for item in items {
                assert!(module.contains(item), "missing {item}");
            }
        }
        // One raised exception is a plain pattern, several an or-pattern.
        let single = with_checked(
            "interface a_v1 { exception e { card32 x; } f() raises (e); }",
            |checked| emit::client_module(checked, "a_v1.if", "a"),
        );
        syn::parse_file(&single).expect("generated code parses");
        assert!(
            single.contains("Some(exception @ Exception::E { .. })"),
            "{single}"
        );
        assert!(client_module.contains("exception @ (Exception::FirmwareError { .. }"));
        // Each function's export entry is named after the interface.
        assert!(interface_module.contains("export_function!(mailbox_v1_get_clock_rate"));
    }
}
