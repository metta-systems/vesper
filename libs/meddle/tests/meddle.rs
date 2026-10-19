//! Tests of the Meddle parser, checks and generator.

#[cfg(test)]
mod tests {
    use {
        libmeddle::{
            LinkSummary, ProviderSummary, SourceFile,
            ast::{Interface, Link, Provider},
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
        check_messages_with(text, &[])
    }

    /// As `check_messages`, with the interfaces in `others` found for `installs`.
    fn check_messages_with(text: &str, others: &[&str]) -> Vec<String> {
        let interface = interface(text);
        let others: Vec<Interface> = others.iter().map(|other| self::interface(other)).collect();
        match check::check(&interface, &others) {
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
        let checked = check::check(&interface, &[])
            .unwrap_or_else(|diagnostics| panic!("unexpected check errors: {diagnostics:?}"));
        use_checked(&checked)
    }

    fn type_code(text: &str) -> u64 {
        type_code_with(text, &[])
    }

    fn type_code_with(text: &str, others: &[&str]) -> u64 {
        let interface = interface(text);
        let others: Vec<Interface> = others.iter().map(|other| self::interface(other)).collect();
        check::check(&interface, &others).map_or(0, |checked| checked.type_code())
    }

    #[test]
    fn mailbox_interface_parses_with_its_documentation() {
        let mailbox = interface(MAILBOX);
        assert_eq!(mailbox.name.text, "mailbox_v1");
        assert!(mailbox.local);
        assert_eq!(mailbox.types.len(), 2);
        assert_eq!(mailbox.exceptions.len(), 3);
        assert_eq!(mailbox.methods.len(), 7);
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
            let set_led = &checked.interface().methods[5];
            assert_eq!(checked.words_of_fields(&set_led.parameters), 1);
            let property = &checked.interface().methods[6];
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
        let mailbox = libmeddle::summarize_interface(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../drivers/mailbox/interfaces/mailbox_v1.if"
            ),
            &[],
        )
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
            "interface a_v1 { f(); } uses b_v1 via device links { g }",
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
            "uses mailbox_v1 via device mboxes { get_clock_rate, set_power_state }",
        );
        let uses = imports.imports(&directories).expect("valid imports");
        assert_eq!(uses[0].functions.len(), 2);

        let imports = source(
            "imports.if",
            "uses mailbox_v1 via device mboxes { get_clock_rate, reboot }",
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
            "uses mailbox_v1 via device firmware, mboxes { get_clock_rate }\n\
             uses mailbox_v1 via device \"raspberrypi,firmware\", mboxes { get_board_serial }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let file = file.expect("parsed uses");
        let links: Vec<Vec<&str>> = file
            .uses
            .iter()
            .map(|uses| match &uses.provider {
                Provider::Device(links) => {
                    links.iter().map(|link| link.name().text.as_str()).collect()
                }
                Provider::Component(name) => panic!("a device chain, not `{}`", name.text),
            })
            .collect();
        assert_eq!(
            links,
            [["firmware", "mboxes"], ["raspberrypi,firmware", "mboxes"]]
        );

        // A chain can start at the one node with a compatible string.
        let (file, diagnostics) = parse(
            "uses mailbox_v1 via device compatible \"raspberrypi,bcm2835-firmware\", mboxes { get_board_serial }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let file = file.expect("parsed uses");
        assert!(matches!(
            &file.uses[0].provider,
            Provider::Device(links)
                if matches!(
                    links.as_slice(),
                    [Link::Compatible(first), Link::Property(second)]
                        if first.text == "raspberrypi,bcm2835-firmware" && second.text == "mboxes"
                )
        ));
        let directories = [PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../drivers/mailbox/interfaces"
        ))];
        let misplaced = source(
            "imports.if",
            "uses mailbox_v1 via device firmware, compatible \"x\" { get_board_serial }",
        );
        let failure = misplaced
            .imports(&directories)
            .expect_err("a compatible link after the first");
        assert!(
            failure.diagnostics[0]
                .message
                .contains("can only start a chain")
        );

        // A use names its provider after `via`.
        let (_, diagnostics) = parse("uses mailbox_v1 { get_clock_rate }");
        assert!(
            diagnostics[0].message.contains("expected `via`"),
            "{diagnostics:?}"
        );

        let imports = source("imports.if", "uses timer_v1 via device timers { now }");
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
        let mailbox =
            libmeddle::summarize_interface(&interfaces[0], &[]).expect("mailbox_v1 summary");
        assert_eq!(mailbox.functions.len(), 7);
        assert_eq!(
            mailbox.export_name(2).as_deref(),
            Some("mailbox_v1_get_clock_rate")
        );

        let imports = std::env::temp_dir().join(format!("meddle-imports-{}", std::process::id()));
        std::fs::create_dir_all(&imports).expect("a scratch directory");
        let imports_file = imports.join("imports.if");
        std::fs::write(
            &imports_file,
            "uses mailbox_v1 via device \"raspberrypi,firmware\" { set_power_state, get_clock_rate }",
        )
        .expect("the imports file");
        let summaries =
            libmeddle::summarize_imports(&imports_file, std::slice::from_ref(&directory))
                .expect("valid imports");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].interface.type_code, mailbox.type_code);
        // In the order `uses` names them, as declaration indices.
        assert_eq!(summaries[0].functions, [4, 2]);
        assert_eq!(
            summaries[0].provider,
            ProviderSummary::Device(vec![LinkSummary::Property(
                "raspberrypi,firmware".to_owned()
            )])
        );

        // A component provider names the manifest component.
        std::fs::write(
            &imports_file,
            "uses mailbox_v1 via component fb { property }",
        )
        .expect("the imports file");
        let summaries =
            libmeddle::summarize_imports(&imports_file, std::slice::from_ref(&directory))
                .expect("valid imports");
        std::fs::remove_dir_all(&imports).ok();
        assert_eq!(
            summaries[0].provider,
            ProviderSummary::Component("fb".to_owned())
        );
    }

    #[test]
    fn providers_are_device_chains_or_components() {
        // The old spellings, without `device`, are not accepted.
        for old in [
            "uses mailbox_v1 via firmware, mboxes { get_clock_rate }",
            "uses mailbox_v1 via compatible \"x\", mboxes { get_clock_rate }",
            "uses mailbox_v1 via mboxes { get_clock_rate }",
        ] {
            let (_, diagnostics) = parse(old);
            assert!(
                diagnostics[0].message.contains("`device` or `component`"),
                "{old}: {diagnostics:?}"
            );
        }

        // `component` takes exactly one name: no chain follows it.
        let (_, diagnostics) = parse("uses mailbox_v1 via component fb, mboxes { get_clock_rate }");
        assert!(
            !diagnostics.is_empty(),
            "a component provider takes no chain"
        );

        // `device` and `component` stay ordinary identifiers outside `via`:
        // mailbox_v1 declares an enum named `device`.
        let (file, diagnostics) = parse(
            "interface device_v1 { enum device { a, b } get_power_state(device which) returns (boolean on); }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(file.is_some());
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

    /// The generated source with all whitespace removed, so assertions do not
    /// depend on the formatter's line breaks.
    fn squash(source: &str) -> String {
        source.split_whitespace().collect()
    }

    const LENDING: &str = "local interface a_v1 {
        placement 2;
        exception e { card32 x; }
        ## Install `count` things into the table the caller lends.
        install(card32 count) lends (key_table into) returns (card32 first) raises (e);
        plain(card32 value) returns (card32 doubled);
    }";

    #[test]
    fn lends_and_placement_parse_into_the_interface() {
        let interface = interface(LENDING);
        let placement = interface.placement.as_ref().expect("a placement window");
        assert_eq!(placement.slots, 2);
        let install = &interface.methods[0];
        assert_eq!(install.lends.len(), 1);
        assert_eq!(install.lends[0].kind.text, "key_table");
        assert_eq!(install.lends[0].name.text, "into");
        assert_eq!(install.parameters.len(), 1);
        assert_eq!(install.results.len(), 1);
        assert_eq!(install.raises.len(), 1);
        assert!(interface.methods[1].lends.is_empty());
        with_checked(LENDING, |checked| {
            assert_eq!(checked.placement_slots(), 2);
            let (kind, name) = checked
                .lent_key(&checked.interface().methods[0])
                .expect("a key");
            assert_eq!(
                (kind, name.text.as_str()),
                (check::LendKind::KeyTable, "into")
            );
            assert!(checked.lent_key(&checked.interface().methods[1]).is_none());
        });
    }

    #[test]
    fn lends_clause_follows_the_parameters() {
        // The clause sits between the parameters and `returns`; elsewhere it
        // is a syntax error, and a key is not a parameter.
        for wrong in [
            "local interface a_v1 { placement 1; f() returns (card32 x) lends (frame page); }",
            "local interface a_v1 { placement 1; f(lends frame page); }",
            "local interface a_v1 { placement 1; f() lends frame page; }",
            "local interface a_v1 { placement 1; f() lends (); }",
        ] {
            let (_, diagnostics) = parse(wrong);
            assert!(!diagnostics.is_empty(), "{wrong} must not parse");
        }
        for fine in [
            "local interface a_v1 { placement 1; f() lends (frame page); }",
            "local interface a_v1 { placement 1; f(card32 x) lends (frame page) raises (e); exception e {} }",
        ] {
            let (_, diagnostics) = parse(fine);
            assert!(diagnostics.is_empty(), "{fine}: {diagnostics:?}");
        }
    }

    #[test]
    fn lent_keys_are_checked() {
        assert!(check_messages(LENDING).is_empty());
        assert_reports(
            "local interface a_v1 { placement 1; f() lends (untyped memory); }",
            "unknown kind of key `untyped`; a function lends a `key_table` or a `frame`",
        );
        assert_reports(
            "local interface a_v1 { placement 1; f() lends (frame one, key_table two); }",
            "lends more than one key",
        );
        assert_reports(
            "interface a_v1 { placement 1; f() lends (frame page); }",
            "`lends` needs a `local` interface",
        );
        assert_reports(
            "local interface a_v1 { placement 1; f(card32 page) lends (frame page); }",
            "parameter `page` is defined twice",
        );
        assert_reports(
            "local interface a_v1 { placement 1; f() lends (frame match); }",
            "reserved word in Rust",
        );
    }

    #[test]
    fn placement_windows_go_with_lending_functions() {
        assert_reports(
            "local interface a_v1 { f() lends (frame page); }",
            "need a `placement <slots>;` window",
        );
        assert_reports(
            "local interface a_v1 { placement 1; f(card32 value); }",
            "no function has a `lends` clause",
        );
        assert_reports(
            "local interface a_v1 { placement 0; f() lends (frame page); }",
            "at least one slot",
        );
        assert_reports(
            "local interface a_v1 { placement 9; f() lends (frame page); }",
            "at most 8 slots",
        );
        assert!(
            check_messages("local interface a_v1 { placement 8; f() lends (frame page); }")
                .is_empty()
        );
    }

    #[test]
    fn lending_is_part_of_the_type_code() {
        let base = type_code("local interface a_v1 { placement 1; f() lends (frame page); }");
        assert_ne!(
            base,
            type_code("local interface a_v1 { placement 2; f() lends (frame page); }")
        );
        assert_ne!(
            base,
            type_code("local interface a_v1 { placement 1; f() lends (key_table page); }")
        );
        assert_ne!(
            base,
            type_code("local interface a_v1 { placement 1; f() lends (frame other); }")
        );
        assert_ne!(
            base,
            type_code("local interface a_v1 { placement 1; f() lends (frame page); g(); }")
        );
        // An interface that lends nothing keeps the code it always had.
        assert_eq!(
            type_code("interface a_v1 { f(card32 x); }"),
            type_code("interface a_v1 { f(card32 x); }")
        );
    }

    const INSTALLING: &str = "local interface a_v1 {
        data 4096;
        placement 1;
        install(card32 first_slot, card16 device) lends (key_table into) installs (b_v1)
            returns (card32 handle);
    }";

    /// The interface `INSTALLING` installs.
    const INSTALLED: &str = "local interface b_v1 { data 4096; badged; f(octet& stage); }";

    #[test]
    fn installs_clause_has_requirements() {
        assert!(check_messages_with(INSTALLING, &[INSTALLED]).is_empty());
        let installing = |text: &str| check_messages_with(text, &[INSTALLED]);
        let reports = |text: &str, expected: &str| {
            let messages = installing(text);
            assert!(
                messages.iter().any(|message| message.contains(expected)),
                "expected {expected:?}, got {messages:?}"
            );
        };
        reports(
            "local interface a_v1 { placement 1; f(card32 first_slot) lends (key_table into) installs (b_v1); }",
            "installing keys need a `data <bytes>;` area",
        );
        reports(
            "interface a_v1 { install(card32 first_slot) installs (b_v1); }",
            "`installs` needs a `local` interface",
        );
        reports(
            "local interface a_v1 { placement 1; f(card32 first_slot) lends (frame page) installs (b_v1); }",
            "must lend a `key_table`",
        );
        reports(
            "local interface a_v1 { placement 1; f() lends (key_table into) installs (b_v1); }",
            "must take a `card32 first_slot` parameter",
        );
        reports(
            "local interface a_v1 { placement 1; f(card16 first_slot) lends (key_table into) installs (b_v1); }",
            "must take a `card32 first_slot` parameter",
        );
        reports(
            "local interface a_v1 { placement 1; f(card32 first_slot) lends (key_table into) installs (a_v1); }",
            "cannot install its own keys",
        );
    }

    #[test]
    fn the_installed_interface_is_found_and_cross_checked() {
        // Nothing found: the include directories did not hold it.
        let missing = check_messages(INSTALLING);
        assert!(
            missing
                .iter()
                .any(|message| message.contains("cannot find `b_v1.if`")),
            "{missing:?}"
        );
        // Found, but not fit for installing.
        for (installed, expected) in [
            (
                "interface b_v1 { data 4096; badged; f(octet& stage); }",
                "`b_v1` must be a `local` interface",
            ),
            (
                "local interface b_v1 { data 4096; f(octet& stage); }",
                "must say `badged;`",
            ),
            (
                "local interface b_v1 { data 8192; badged; f(octet& stage); }",
                "they share one area",
            ),
            (
                "local interface b_v1 { badged; f(); }",
                "they share one area",
            ),
            (
                "local interface b_v1 { data 4096; badged; }",
                "has no functions to install",
            ),
        ] {
            let messages = check_messages_with(INSTALLING, &[installed]);
            assert!(
                messages.iter().any(|message| message.contains(expected)),
                "{installed}: expected {expected:?}, got {messages:?}"
            );
        }
    }

    #[test]
    fn installed_interface_is_part_of_the_type_code() {
        let base = type_code_with(INSTALLING, &[INSTALLED]);
        assert_ne!(base, 0);
        // The installing interface changes with the interface it installs.
        let grown = "local interface b_v1 { data 4096; badged; f(octet& stage); g(); }";
        assert_ne!(base, type_code_with(INSTALLING, &[grown]));
        // And with the name of it.
        let renamed = INSTALLING.replace("b_v1", "c_v1");
        let other = INSTALLED.replace("b_v1", "c_v1");
        assert_ne!(base, type_code_with(&renamed, &[&other]));
        assert_ne!(base, type_code(&INSTALLING.replace("installs (b_v1)", "")));
    }

    #[test]
    fn exports_list_their_entries_and_clients_bind_installed_keys() {
        let source = "local interface p_v1 { data 4096; first(); second(octet& stage); }";
        let (interface_module, client_module) = with_checked(source, |checked| {
            (
                squash(&emit::interface_module(checked, "p_v1.if")),
                squash(&emit::client_module(checked, "p_v1.if", "p")),
            )
        });
        let (raw_interface, raw_client) = with_checked(source, |checked| {
            (
                emit::interface_module(checked, "p_v1.if"),
                emit::client_module(checked, "p_v1.if", "p"),
            )
        });
        syn::parse_file(&raw_interface).expect("generated interface code parses");
        syn::parse_file(&raw_client).expect("generated client code parses");
        assert!(interface_module.contains("pubconstFUNCTION_COUNT:usize=2;"));
        assert!(interface_module.contains("pubfnp_v1_entries()->[u64;2]"));
        assert!(interface_module.contains("[p_v1_firstasusizeasu64,p_v1_secondasusizeasu64]"));
        assert!(client_module.contains("pubfnfrom_installed("));
        assert!(client_module.contains("let[key_0,key_1]=*keyselse"));
    }

    #[test]
    fn channels_are_local_and_bounded_and_part_of_the_type_code() {
        let source = "local interface a_v1 { channel 2; f(); }";
        assert!(check_messages(source).is_empty());
        assert_reports(
            "interface a_v1 { channel 2; f(); }",
            "`channel` needs a `local` interface",
        );
        for pages in [0, 17] {
            assert_reports(
                &format!("local interface a_v1 {{ channel {pages}; f(); }}"),
                "a channel has from 1 to 16 data pages",
            );
        }
        assert!(check_messages("local interface a_v1 { channel 16; f(); }").is_empty());
        let plain = "local interface a_v1 { f(); }";
        assert_ne!(type_code(source), type_code(plain));
        assert_ne!(
            type_code(source),
            type_code("local interface a_v1 { channel 3; f(); }")
        );
    }

    #[test]
    fn a_channel_reaches_the_generated_constants_and_bindings() {
        let source = "local interface a_v1 { channel 2; f(); }";
        let (interface_module, client_module) = with_checked(source, |checked| {
            (
                squash(&emit::interface_module(checked, "a_v1.if")),
                squash(&emit::client_module(checked, "a_v1.if", "a")),
            )
        });
        let (raw_interface, raw_client) = with_checked(source, |checked| {
            (
                emit::interface_module(checked, "a_v1.if"),
                emit::client_module(checked, "a_v1.if", "a"),
            )
        });
        syn::parse_file(&raw_interface).expect("generated interface code parses");
        syn::parse_file(&raw_client).expect("generated client code parses");
        assert!(interface_module.contains("pubconstCHANNEL_PAGES:u64=2;"));
        assert!(client_module.contains("pubchannel:Option<__runtime::ChannelArea>"));
        assert!(client_module.contains("bindings.channel=record.channel(CHANNEL_PAGES);"));
        assert!(client_module.contains("pubconstfnchannel(&self)"));
    }

    #[test]
    fn badged_interfaces_pass_the_badge_to_every_function() {
        assert_reports(
            "interface a_v1 { badged; f(); }",
            "`badged` needs a `local` interface",
        );
        let source = "local interface a_v1 { badged; f(card32 value); }";
        let plain = "local interface a_v1 { f(card32 value); }";
        assert_ne!(type_code(source), type_code(plain));
        let interface_module = with_checked(source, |checked| {
            squash(&emit::interface_module(checked, "a_v1.if"))
        });
        assert!(interface_module.contains("fnf(badge:u64,value:u32)"));
        assert!(interface_module.contains("Implementation::f(badge,value)"));
        let plain_module = with_checked(plain, |checked| {
            squash(&emit::interface_module(checked, "a_v1.if"))
        });
        assert!(plain_module.contains("fnf(value:u32)"));
    }

    #[test]
    fn lending_functions_generate_typed_code() {
        let (interface_module, client_module) = with_checked(LENDING, |checked| {
            (
                squash(&emit::interface_module(checked, "a_v1.if")),
                squash(&emit::client_module(checked, "a_v1.if", "a")),
            )
        });
        let (raw_interface, raw_client) = with_checked(LENDING, |checked| {
            (
                emit::interface_module(checked, "a_v1.if"),
                emit::client_module(checked, "a_v1.if", "a"),
            )
        });
        syn::parse_file(&raw_interface).expect("generated interface code parses");
        syn::parse_file(&raw_client).expect("generated client code parses");

        // The lent key comes first, typed by its kind and borrowed.
        assert!(interface_module.contains("fninstall(into:&__runtime::KeyTableKey,count:u32"));
        assert!(
            client_module.contains("pubfninstall(&self,into:&__runtime::KeyTableKey,count:u32")
        );
        // It is lent through CallWithKey, outside the argument words.
        assert!(client_module.contains("call_with_key(into.raw(),arguments)"));
        assert!(client_module.contains("binding.call(arguments)"));
        // The dispatcher receives it first and the export uses the lending macro.
        assert!(
            interface_module
                .contains("lent:__runtime::RawKey,arguments:[u64;__runtime::ARGUMENT_WORDS]")
        );
        assert!(interface_module.contains("letinto=__runtime::KeyTableKey::from_key(lent);"));
        assert!(interface_module.contains("Implementation::install(&into,count)"));
        assert!(interface_module.contains("export_function_with_key!(a_v1_install"));
        assert!(interface_module.contains("export_function!(a_v1_plain"));
        assert!(interface_module.contains("pubconstPLACEMENT_SLOTS:u64=2;"));
        assert!(client_module.contains("PLACEMENT_SLOTS"));

        // Frames are typed as frames.
        let frames = with_checked(
            "local interface b_v1 { placement 1; f() lends (frame page); }",
            |checked| squash(&emit::interface_module(checked, "b_v1.if")),
        );
        assert!(frames.contains("fnf(page:&__runtime::FrameKey)"));
        assert!(frames.contains("letpage=__runtime::FrameKey::from_key(lent);"));
    }
}
