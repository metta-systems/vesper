pub struct AArch64PageSizes;

impl PageSizeSupport for AArch64PageSizes {
    fn supported_sizes(&self) -> &[PageSize] {
        // AArch64 supports 4KB, 16KB, 64KB base pages and larger huge pages
        static SIZES: &[PageSize] = &[
            PageSize::new(4 * 1024, 0),        // 4KB
            PageSize::new(16 * 1024, 0),       // 16KB
            PageSize::new(64 * 1024, 0),       // 64KB
            PageSize::new(512 * 1024, 1),      // 512KB
            PageSize::new(1024 * 1024, 1),     // 1MB
            PageSize::new(2 * 1024 * 1024, 2), // 2MB
        ];
        SIZES
    }
}
