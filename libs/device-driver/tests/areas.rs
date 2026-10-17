//! Host tests of the data areas an exporting driver serves.

#[cfg(test)]
mod tests {
    use vesper_device_driver::{DATA_WINDOW_VA, DriverInit, ExportedArea, MAX_AREAS};

    fn area(index: u64) -> ExportedArea {
        // Stacks 0x1_0000 bytes apart, areas one page each in the window.
        ExportedArea {
            stack_bottom: 0x3000_0000 + index * 0x1_0000,
            stack_end: 0x3000_0000 + index * 0x1_0000 + 0x4000,
            base: DATA_WINDOW_VA + index * 0x1000,
            len: 0x1000,
        }
    }

    fn init_with_areas(count: u64) -> DriverInit {
        let mut init = DriverInit::EMPTY;
        for index in 0..count {
            init.areas[usize::try_from(index).unwrap()] = area(index);
        }
        init.area_count = count;
        init
    }

    #[test]
    fn a_stack_pointer_selects_the_area_of_the_stack_it_is_in() {
        let init = init_with_areas(3);
        for index in 0..3 {
            let stack = area(index);
            // The initial stack pointer is the end of the stack; a running
            // call is somewhere below it, above the bottom.
            for stack_pointer in [
                stack.stack_end,
                stack.stack_end - 0x80,
                stack.stack_bottom + 1,
            ] {
                assert_eq!(
                    init.area_for_stack(stack_pointer),
                    Some(stack),
                    "{stack_pointer:#x}"
                );
            }
        }
    }

    #[test]
    fn stack_pointers_outside_every_stack_select_nothing() {
        let init = init_with_areas(2);
        let first = area(0);
        for stack_pointer in [
            0,
            first.stack_bottom,
            first.stack_end + 1,
            area(2).stack_end,
            u64::MAX,
        ] {
            assert_eq!(
                init.area_for_stack(stack_pointer),
                None,
                "{stack_pointer:#x}"
            );
        }
        assert_eq!(DriverInit::EMPTY.area_for_stack(first.stack_end), None);
    }

    #[test]
    fn the_area_count_is_clamped_to_the_table() {
        let mut init = init_with_areas(MAX_AREAS as u64);
        init.area_count = u64::MAX;
        assert_eq!(init.areas().len(), MAX_AREAS);
    }
}
