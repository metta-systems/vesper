/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

// L0 table
struct PageGlobalDirectory {
    // @todo should also impl VirtSpace to be able to map shit?
// or the Page's impl will do this?
}

impl PageCacheManagement for PageGlobalDirectory {
    fn clean_data(start_offset: usize, end_offset: usize) -> ! {
        todo!()
    }

    fn clean_invalidate_data(start_offset: usize, end_offset: usize) -> ! {
        todo!()
    }

    fn invalidate_data(start_offset: usize, end_offset: usize) -> ! {
        todo!()
    }

    fn unify_instruction_cache(start_offset: usize, end_offset: usize) -> ! {
        todo!()
    }
}
