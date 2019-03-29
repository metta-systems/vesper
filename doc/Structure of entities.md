Structure of entities (local scope architecture)

Current:
```
vesper : crate
 ├── arch : public
 │   ├── aarch64 : public @ #[cfg(target_arch = "aarch64")]
 │   │   ├── boot
 │   │   ├── gic
 │   │   ├── mmu (@todo move MMU code to memory module directly)
 │   │   ├── memory
 │   │   │   ├── addr::{PhysAddr, PhysAddrNotValid, VirtAddr, VirtAddrNotValid}
 │   │   │   ├── area_frame_allocator::AreaFrameAllocator
 │   │   │   ├── bump_allocator::BumpAllocator
 │   │   │   └── paging
 │   │   │       ├── entry::Entry
 │   │   │       ├── frame::{PhysFrame<S>, PhysFrameRange<S>}
 │   │   │       ├── page::Page
 │   │   │       └── table::??
 │   │   ├── regs
 │   │   │   ├── esr_el1::ESR_EL1
 │   │   │   ├── far_el1::FAR_EL1
 │   │   │   ├── id_aa64mmfr0_el1::ID_AA64MMFR0_EL1
 │   │   │   └── macros
 │   │   └── traps
 │   └── x86_64 : public @ #[cfg(target_arch = "x86_64")]
 │       └── boot
 ├── devices : private
 │   └── console : public
 ├── jlink_rtt : private (@todo Move to devices)
 ├── macros : private          <-- @todo move this somewhere??
 ├── platform : public
 │   └── rpi3 : public
 │       ├── display : public
 │       ├── gpio : public
 │       ├── mailbox : public
 │       ├── mini_uart : public
 │       ├── power : public
 │       ├── uart : public
 │       └── vc : public
 ├── sync : private             <-- @todo move this somewhere
 └── write_to : private         <-- @todo move this somewhere
```

Wanted:
```
```
