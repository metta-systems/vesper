extern "C" {
    fn atoi(__nptr: *const ::core::ffi::c_char) -> ::core::ffi::c_int;
    fn malloc(__size: size_t) -> *mut ::core::ffi::c_void;
    fn printf(__format: *const ::core::ffi::c_char, ...) -> ::core::ffi::c_int;
    fn snprintf(
        __s: *mut ::core::ffi::c_char,
        __maxlen: size_t,
        __format: *const ::core::ffi::c_char,
        ...
    ) -> ::core::ffi::c_int;
    fn putchar(__c: ::core::ffi::c_int) -> ::core::ffi::c_int;
    fn puts(__s: *const ::core::ffi::c_char) -> ::core::ffi::c_int;
    fn strlen(__s: *const ::core::ffi::c_char) -> size_t;
    fn strcasecmp(
        __s1: *const ::core::ffi::c_char,
        __s2: *const ::core::ffi::c_char,
    ) -> ::core::ffi::c_int;
    static mut myargc: ::core::ffi::c_int;
    static mut myargv: *mut *mut ::core::ffi::c_char;
    fn M_CheckParmWithArgs(
        check: *mut ::core::ffi::c_char,
        num_args: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int;
    fn M_StrToInt(str: *const ::core::ffi::c_char, result: *mut ::core::ffi::c_int) -> boolean;
    fn I_ErrorV(msg: *const ::core::ffi::c_char);
}
pub type size_t = usize;
pub type __uint8_t = u8;
pub type uint8_t = __uint8_t;
pub type boolean = ::core::ffi::c_uint;
pub type byte = uint8_t;
pub type atexit_func_t = Option<unsafe extern "C" fn() -> ()>;
pub type atexit_listentry_t = atexit_listentry_s;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct atexit_listentry_s {
    pub func: atexit_func_t,
    pub run_on_error: boolean,
    pub next: *mut atexit_listentry_t,
}
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const true_0: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
pub const false_0: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
pub const DEFAULT_RAM: ::core::ffi::c_int = 6 as ::core::ffi::c_int;
pub const MIN_RAM: ::core::ffi::c_int = 6 as ::core::ffi::c_int;
static mut exit_funcs: *mut atexit_listentry_t = ::core::ptr::null_mut::<atexit_listentry_t>();
#[no_mangle]
pub unsafe extern "C" fn I_AtExit(mut func: atexit_func_t, mut run_on_error: boolean) {
    let mut entry: *mut atexit_listentry_t = ::core::ptr::null_mut::<atexit_listentry_t>();
    entry =
        malloc(::core::mem::size_of::<atexit_listentry_t>() as size_t) as *mut atexit_listentry_t;
    (*entry).func = func;
    (*entry).run_on_error = run_on_error;
    (*entry).next = exit_funcs;
    exit_funcs = entry;
}
#[no_mangle]
pub unsafe extern "C" fn I_Tactile(
    mut on: ::core::ffi::c_int,
    mut off: ::core::ffi::c_int,
    mut total: ::core::ffi::c_int,
) {
}
unsafe extern "C" fn AutoAllocMemory(
    mut size: *mut ::core::ffi::c_int,
    mut default_ram: ::core::ffi::c_int,
    mut min_ram: ::core::ffi::c_int,
) -> *mut byte {
    let mut zonemem: *mut byte = ::core::ptr::null_mut::<byte>();
    zonemem = ::core::ptr::null_mut::<byte>();
    while zonemem.is_null() {
        if default_ram < min_ram {
            let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
            snprintf(
                &raw mut _i_err_buf as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                b"Unable to allocate %i MiB of RAM for zone\0".as_ptr()
                    as *const ::core::ffi::c_char,
                default_ram,
            );
            I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
        }
        *size = default_ram * 1024 as ::core::ffi::c_int * 1024 as ::core::ffi::c_int;
        zonemem = malloc(*size as size_t) as *mut byte;
        if zonemem.is_null() {
            default_ram -= 1 as ::core::ffi::c_int;
        }
    }
    return zonemem;
}
#[no_mangle]
pub unsafe extern "C" fn I_ZoneBase(mut size: *mut ::core::ffi::c_int) -> *mut byte {
    let mut zonemem: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut min_ram: ::core::ffi::c_int = 0;
    let mut default_ram: ::core::ffi::c_int = 0;
    let mut p: ::core::ffi::c_int = 0;
    p = M_CheckParmWithArgs(
        b"-mb\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        1 as ::core::ffi::c_int,
    );
    if p > 0 as ::core::ffi::c_int {
        default_ram = atoi(*myargv.offset((p + 1 as ::core::ffi::c_int) as isize));
        min_ram = default_ram;
    } else {
        default_ram = DEFAULT_RAM;
        min_ram = MIN_RAM;
    }
    zonemem = AutoAllocMemory(size, default_ram, min_ram);
    printf(
        b"zone memory: %p, %x allocated for zone\n\0".as_ptr() as *const ::core::ffi::c_char,
        zonemem,
        *size,
    );
    return zonemem;
}
#[no_mangle]
pub unsafe extern "C" fn I_PrintBanner(mut msg: *mut ::core::ffi::c_char) {
    let mut i: ::core::ffi::c_int = 0;
    let mut spaces: ::core::ffi::c_int =
        (35 as size_t).wrapping_sub(strlen(msg).wrapping_div(2 as size_t)) as ::core::ffi::c_int;
    i = 0 as ::core::ffi::c_int;
    while i < spaces {
        putchar(' ' as ::core::ffi::c_int);
        i += 1;
    }
    puts(msg);
}
#[no_mangle]
pub unsafe extern "C" fn I_PrintDivider() {
    let mut i: ::core::ffi::c_int = 0;
    i = 0 as ::core::ffi::c_int;
    while i < 75 as ::core::ffi::c_int {
        putchar('=' as ::core::ffi::c_int);
        i += 1;
    }
    putchar('\n' as ::core::ffi::c_int);
}
#[no_mangle]
pub unsafe extern "C" fn I_PrintStartupBanner(mut gamedescription: *mut ::core::ffi::c_char) {
    I_PrintDivider();
    I_PrintBanner(gamedescription);
    I_PrintDivider();
    printf(
        b" Doom Generic is free software, covered by the GNU General Public\n License.  There is NO warranty; not even for MERCHANTABILITY or FITNESS\n FOR A PARTICULAR PURPOSE. You are welcome to change and distribute\n copies under certain conditions. See the source for more information.\n\0"
            .as_ptr() as *const ::core::ffi::c_char,
    );
    I_PrintDivider();
}
#[no_mangle]
pub unsafe extern "C" fn I_ConsoleStdout() -> boolean {
    return 0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn I_Quit() {
    let mut entry: *mut atexit_listentry_t = ::core::ptr::null_mut::<atexit_listentry_t>();
    entry = exit_funcs;
    while !entry.is_null() {
        (*entry).func.expect("non-null function pointer")();
        entry = (*entry).next;
    }
}
pub const DOS_MEM_DUMP_SIZE: ::core::ffi::c_int = 10 as ::core::ffi::c_int;
static mut mem_dump_dos622: [::core::ffi::c_uchar; 10] = [
    0x57 as ::core::ffi::c_uchar,
    0x92 as ::core::ffi::c_uchar,
    0x19 as ::core::ffi::c_uchar,
    0 as ::core::ffi::c_uchar,
    0xf4 as ::core::ffi::c_uchar,
    0x6 as ::core::ffi::c_uchar,
    0x70 as ::core::ffi::c_uchar,
    0 as ::core::ffi::c_uchar,
    0x16 as ::core::ffi::c_uchar,
    0 as ::core::ffi::c_uchar,
];
static mut mem_dump_win98: [::core::ffi::c_uchar; 10] = [
    0x9e as ::core::ffi::c_uchar,
    0xf as ::core::ffi::c_uchar,
    0xc9 as ::core::ffi::c_uchar,
    0 as ::core::ffi::c_uchar,
    0x65 as ::core::ffi::c_uchar,
    0x4 as ::core::ffi::c_uchar,
    0x70 as ::core::ffi::c_uchar,
    0 as ::core::ffi::c_uchar,
    0x16 as ::core::ffi::c_uchar,
    0 as ::core::ffi::c_uchar,
];
static mut mem_dump_dosbox: [::core::ffi::c_uchar; 10] = [
    0 as ::core::ffi::c_uchar,
    0 as ::core::ffi::c_uchar,
    0 as ::core::ffi::c_uchar,
    0xf1 as ::core::ffi::c_uchar,
    0 as ::core::ffi::c_uchar,
    0 as ::core::ffi::c_uchar,
    0 as ::core::ffi::c_uchar,
    0 as ::core::ffi::c_uchar,
    0x7 as ::core::ffi::c_uchar,
    0 as ::core::ffi::c_uchar,
];
static mut mem_dump_custom: [::core::ffi::c_uchar; 10] = [0; 10];
static mut dos_mem_dump: *const ::core::ffi::c_uchar =
    unsafe { &raw const mem_dump_dos622 as *const ::core::ffi::c_uchar };
#[no_mangle]
pub unsafe extern "C" fn I_GetMemoryValue(
    mut offset: ::core::ffi::c_uint,
    mut value: *mut ::core::ffi::c_void,
    mut size: ::core::ffi::c_int,
) -> boolean {
    static mut firsttime: boolean = true_0 as boolean;
    if firsttime != 0 {
        let mut p: ::core::ffi::c_int = 0;
        let mut i: ::core::ffi::c_int = 0;
        let mut val: ::core::ffi::c_int = 0;
        firsttime = false_0 as boolean;
        i = 0 as ::core::ffi::c_int;
        p = M_CheckParmWithArgs(
            b"-setmem\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            1 as ::core::ffi::c_int,
        );
        if p > 0 as ::core::ffi::c_int {
            if strcasecmp(
                *myargv.offset((p + 1 as ::core::ffi::c_int) as isize),
                b"dos622\0".as_ptr() as *const ::core::ffi::c_char,
            ) == 0
            {
                dos_mem_dump = &raw const mem_dump_dos622 as *const ::core::ffi::c_uchar;
            }
            if strcasecmp(
                *myargv.offset((p + 1 as ::core::ffi::c_int) as isize),
                b"dos71\0".as_ptr() as *const ::core::ffi::c_char,
            ) == 0
            {
                dos_mem_dump = &raw const mem_dump_win98 as *const ::core::ffi::c_uchar;
            } else if strcasecmp(
                *myargv.offset((p + 1 as ::core::ffi::c_int) as isize),
                b"dosbox\0".as_ptr() as *const ::core::ffi::c_char,
            ) == 0
            {
                dos_mem_dump = &raw const mem_dump_dosbox as *const ::core::ffi::c_uchar;
            } else {
                i = 0 as ::core::ffi::c_int;
                while i < DOS_MEM_DUMP_SIZE {
                    p += 1;
                    if p >= myargc
                        || *(*myargv.offset(p as isize)).offset(0 as ::core::ffi::c_int as isize)
                            as ::core::ffi::c_int
                            == '-' as ::core::ffi::c_int
                    {
                        break;
                    }
                    M_StrToInt(*myargv.offset(p as isize), &raw mut val);
                    let c2rust_fresh0 = i;
                    i = i + 1;
                    mem_dump_custom[c2rust_fresh0 as usize] = val as ::core::ffi::c_uchar;
                    i += 1;
                }
                dos_mem_dump = &raw mut mem_dump_custom as *mut ::core::ffi::c_uchar;
            }
        }
    }
    match size {
        1 => {
            *(value as *mut ::core::ffi::c_uchar) = *dos_mem_dump.offset(offset as isize);
            return true_0 as boolean;
        }
        2 => {
            *(value as *mut ::core::ffi::c_ushort) =
                (*dos_mem_dump.offset(offset as isize) as ::core::ffi::c_int
                    | (*dos_mem_dump.offset(offset.wrapping_add(1 as ::core::ffi::c_uint) as isize)
                        as ::core::ffi::c_int)
                        << 8 as ::core::ffi::c_int) as ::core::ffi::c_ushort;
            return true_0 as boolean;
        }
        4 => {
            *(value as *mut ::core::ffi::c_uint) =
                (*dos_mem_dump.offset(offset as isize) as ::core::ffi::c_int
                    | (*dos_mem_dump.offset(offset.wrapping_add(1 as ::core::ffi::c_uint) as isize)
                        as ::core::ffi::c_int)
                        << 8 as ::core::ffi::c_int
                    | (*dos_mem_dump.offset(offset.wrapping_add(2 as ::core::ffi::c_uint) as isize)
                        as ::core::ffi::c_int)
                        << 16 as ::core::ffi::c_int
                    | (*dos_mem_dump.offset(offset.wrapping_add(3 as ::core::ffi::c_uint) as isize)
                        as ::core::ffi::c_int)
                        << 24 as ::core::ffi::c_int) as ::core::ffi::c_uint;
            return true_0 as boolean;
        }
        _ => {}
    }
    return false_0 as boolean;
}
