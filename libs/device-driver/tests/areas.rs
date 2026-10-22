//! Host tests of the data areas an exporting driver serves.

#[cfg(test)]
mod tests {
    use vesper_device_driver::{DATA_WINDOW_VA, DriverInit, ExportedArea, MAX_AREAS};

    fn area(index: u64) -> ExportedArea {
        // Areas one page each in the window.
        ExportedArea {
            base: DATA_WINDOW_VA + index * 0x1000,
            len: 0x1000,
            channel_base: 0,
            channel_event: 0,
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
    fn a_badge_selects_the_client_whose_area_base_it_is() {
        let init = init_with_areas(3);
        for index in 0..3 {
            let expected = area(index);
            assert_eq!(
                init.area_for_badge(expected.base),
                Some((usize::try_from(index).unwrap(), expected)),
                "{:#x}",
                expected.base
            );
        }
    }

    #[test]
    fn badges_that_name_no_area_select_nothing() {
        let init = init_with_areas(2);
        // Zero is an unbadged key; the others are not the base of any area,
        // including the base of an area the exporter does not serve and an
        // address inside a served area.
        for badge in [0, 1, area(0).base + 1, area(2).base, u64::MAX] {
            assert_eq!(init.area_for_badge(badge), None, "{badge:#x}");
        }
        assert_eq!(DriverInit::EMPTY.area_for_badge(area(0).base), None);
    }

    #[test]
    fn the_area_count_is_clamped_to_the_table() {
        let mut init = init_with_areas(MAX_AREAS as u64);
        init.area_count = u64::MAX;
        assert_eq!(init.areas().len(), MAX_AREAS);
    }
}
