# === Configuration ===

target          := 'aarch64-metta-none-eabi'
# ⚠️ Target path must be 'escaped' to work on Windows
target_json     := "-Zjson-target-spec --target='" + justfile_directory() / 'targets' / target + ".json'"
rust_std        := '-Zbuild-std=compiler_builtins,core,alloc -Zbuild-std-features=compiler-builtins-mem'

# Board presets: rustflags, dtb, qemu-machine
board_rpi3_flags  := '-C target-cpu=cortex-a53 --cfg board_rpi3'
board_rpi4_flags  := '-C target-cpu=cortex-a73 --cfg board_rpi4'
rpi3_dtb          := justfile_directory() / 'targets/bcm2710-rpi-3-b-plus.dtb'
rpi4_dtb          := justfile_directory() / 'targets/bcm2711-rpi-4-b.dtb'
# The rpi3 tree with the PWM node enabled for the headphone jack, kept in the repository so
# the tests need no dtc (`just audio-test-dtb` remakes it from targets/overlays/audio-test-pwm.dts,
# as the board's `pwm-2chan` overlay does)
audio_test_dtb    := justfile_directory() / 'targets/audio-test.dtb'

nucleus_link    := 'linker/nucleus.ld'
init_link       := 'linker/kickstart.ld'
user_link       := 'userspace/runtime/user.ld'
test_link       := 'linker/test.ld'
chainboot_link  := 'bin/chainboot/src/link.ld'
privileged_link := 'linker/privileged.ld'

fixed_rustflags := '-D warnings -Z macro-backtrace'
# Privileged (EL1) components: position-independent, linked at 0 and relocated by kickstart
pie_rustflags   := '-C relocation-model=pie -C link-arg=--pie'
# Device tests run in QEMU (rpi3) with the test linker script
device_test_rustflags := fixed_rustflags + ' ' + board_rpi3_flags + ' -C link-arg=--script=' + test_link

# Sort components alphabetically!
# EL0 userspace components, linked with the userspace runtime's user.ld
user_components := 'audio-pwm-bcm2835 audio-test-client compositor cprman-bcm2835 dispatch-actor dispatch-child dispatch-dispatcher dma-bcm2835 driver-probe endpoint-client endpoint-component endpoint-server fault-bare fault-faulter fb-impl fp-probe gpio-bcm2835 hello input-trace irq-receiver mailbox-impl mailbox-test-client power-impl preempt-reader preempt-spinner root-dispatcher sd-test-client sdhost-bcm2835 shell uart-pl011 usb-core usb-hcd-dwc2 usb-hid usb-test-client window-events'
# Privileged components every kickstart image bundles (see kernel/kickstart/image.toml)
privileged_components := 'irqchip-bcm2836 irqchip-gicv2'
# EL0 driver and program components the kickstart image bundles (see kernel/kickstart/image.toml)
driver_components := 'compositor fb-impl mailbox-impl power-impl root-dispatcher shell uart-pl011 usb-core usb-hcd-dwc2 usb-hid'

qemu            := env('QEMU', 'qemu-system-aarch64')
qemu_machine    := env('QEMU_MACHINE', 'raspi3b')
gdb             := env('GDB', 'aarch64-elf-gdb') # An aarch64-enabled GDB (brew install aarch64-elf-gdb)
objcopy         := 'rust-objcopy'
nm              := 'rust-nm'
volume          := env('VOLUME', '/Volumes/BOOT')

# Profile of the cross-built images (`debug` or `release`); Kickstart's build script embeds the
# nucleus and components from this profile's directory (VESPER_IMAGE_PROFILE)
image_profile   := env('VESPER_IMAGE_PROFILE', 'release')
export VESPER_IMAGE_PROFILE := image_profile
# Cargo names the debug profile `dev` (`debug` is reserved); its outputs go to `debug/`
cargo_profile   := if image_profile == 'debug' { 'dev' } else { image_profile }

# Image configurations. Every board/feature/profile combination builds in its own
# target directory, `target/<config>`, named `<board>[-<feature>+<feature>…][-debug]`
# (for example `rpi3-qemu`, `rpi4`, `rpi3-qemu+debug_kernel`), so Kickstart embeds the
# nucleus and components of its own configuration. Boot images are written to
# `target/images/<image>-<config>.bin`.
target_root     := justfile_directory() / 'target'
images_dir      := target_root / 'images'
profile_suffix  := if image_profile == 'debug' { '-debug' } else { '' }
rpi3_qemu       := 'rpi3-qemu' + profile_suffix
rpi4_qemu       := 'rpi4-qemu' + profile_suffix
rpi4_hw         := 'rpi4' + profile_suffix
# Features of the Kickstart-based e2e test kernels unless a test names its own
test_kernel_features := 'qemu,debug_kernel'
# Boot images and ELFs of the configurations the run, debug and SD-card recipes use
kernel_rpi3_qemu_bin    := images_dir / 'kernel-' + rpi3_qemu + '.bin'
kernel_rpi4_qemu_bin    := images_dir / 'kernel-' + rpi4_qemu + '.bin'
kernel_rpi4_bin         := images_dir / 'kernel-' + rpi4_hw + '.bin'
kernel_rpi4_elf         := rpi4_hw_artifacts / image_profile / 'kickstart'
chainboot_rpi3_qemu_bin := images_dir / 'chainboot-' + rpi3_qemu + '.bin'
chainboot_rpi4_bin      := images_dir / 'chainboot-' + rpi4_hw + '.bin'
chainboot_rpi4_elf      := rpi4_hw_artifacts / image_profile / 'chainboot'
# `target/<config>/<triple>`, where a configuration's ELFs live under `<profile>/`
rpi3_qemu_artifacts := target_root / rpi3_qemu / target
rpi4_hw_artifacts   := target_root / rpi4_hw / target

chainboot_serial    := '/dev/ttyUSB0' # '/dev/cu.usbserial-019586E3'
chainboot_baud      := '115200'
# The rate chainofcommand asks chainboot to load the kernel at, and the console stays at
# (e.g. `just chainboot_load_baud=2000000 boot`); empty loads at `chainboot_baud`.
# It also asks Kickstart's console for it when the kernel boots from the SD card (`just console`)
chainboot_load_baud := '1500000'

chainofcommand_args := chainboot_serial + ' ' + chainboot_baud + (if chainboot_load_baud != '' { ' --load-baud ' + chainboot_load_baud } else { '' })

# QEMU option fragments
# Console, monitor and semihosting wiring shared by every QEMU run
qemu_stdio_opts   := '-chardev stdio,mux=on,id=char0,logfile=qemu.log,signal=off -mon chardev=char0,mode=readline -semihosting-config enable=on,userspace=on,chardev=char0'
qemu_io_opts      := '-serial chardev:char0 ' + qemu_stdio_opts
qemu_base_opts    := '-M ' + qemu_machine + ' ' + qemu_io_opts
qemu_disasm       := '-d in_asm,unimp,int,mmu,cpu_reset,guest_errors,nochain,plugin'
qemu_gdb_opts     := '-gdb tcp::5555 -S'
qemu_test_opts    := '-nographic'
# Guest time counted in executed instructions, not read from the host clock, for the tests
# that check timing bounds: a slow or loaded host cannot stretch what the guest measures
qemu_icount_opts  := '-icount shift=0,sleep=off'
# The root port of QEMU's raspi3b has no device unless one is given, and the DWC2 driver waits for one: a hub with a keyboard behind it
qemu_usb          := '-device usb-hub,port=1 -device usb-kbd,port=1.1'
# Real USB devices forwarded from host
qemu_usb_forward  := '-device usb-hub,id=hub0 -device usb-host,vendorid=0x04d9,productid=0x0006 -device usb-host,vendorid=0x093a,productid=0x2510'
qemu_disasm_gdb   := qemu_disasm + ' ' + qemu_gdb_opts
# Boot a test image (rpi3); in-guest assertions and the QEMU exit status are the result
qemu_test         := qemu + ' ' + qemu_base_opts + ' ' + qemu_test_opts + ' -dtb "' + rpi3_dtb + '"'

gdb_connect     := justfile_directory() / 'target' / target / 'gdb-connect'

openocd_bin     := env('OPENOCD', '/usr/local/opt/openocd/4d6519593-rtt/bin/openocd')

ok_label        := '✅'
copy_label      := '🔄'

# Only the everyday recipes are listed. The private ones (single tests, focused lints,
# hardware test images, debugging variants) still run by name, `just <recipe>`;
# the aggregate recipes (`test`, `test-host`, `clippy`, `lint`) run them all.
_default:
    @just --list

# === Low-level: cross-compile a single crate ===

# Cross-build crates (space-separated) for a board with a linker script, features and extra
# rustflags, into the target directory of the image configuration `board` + `image_features`
# (the features of the image they are bundled into, which may differ from their own)
_cross-build crates board='rpi4' linker_script='' features='' rustflags='' image_features=features:
    CARGO_TARGET_DIR="{{ target_root / (board + (if image_features == '' { '' } else { '-' + replace(image_features, ',', '+') }) + profile_suffix) }}" \
    RUSTFLAGS="{{ fixed_rustflags }} {{ if board == 'rpi3' { board_rpi3_flags } else { board_rpi4_flags } }}{{ if linker_script != '' { ' -C link-arg=--script=' + linker_script } else { '' } }}{{ if rustflags != '' { ' ' + rustflags } else { '' } }}" \
    cargo build {{ target_json }} \
      {{ if features != '' { '--features=' + features } else { '' } }} \
      {{ rust_std }} \
      --profile {{ cargo_profile }} -p {{ replace(crates, ' ', ' -p ') }}

# === Kernel (nucleus + kickstart -> kernel-<config>.bin) ===

# Build the privileged components every kickstart image bundles (see kernel/kickstart/image.toml)
_build-privileged board='rpi4' image_features='': (_cross-build privileged_components board privileged_link '' pie_rustflags image_features)

# Build the EL0 driver components the kickstart image bundles (with semihosting output under 'qemu')
_build-drivers board='rpi4' features='' image_features=features:
    "{{ just_executable() }}" _cross-build '{{ driver_components }}' '{{ board }}' '{{ user_link }}' '{{ if features =~ "qemu" { "qemu" } else { "" } }}' '' '{{ image_features }}'

# Build kernel (features: '' for hw, 'qemu' for emulation) into target/images/kernel-<config>.bin
[group("build")]
build board='rpi4' features='': (_build-kernel board features)

alias b := build

_build-kernel board features config=(board + (if features == '' { '' } else { '-' + replace(features, ',', '+') }) + profile_suffix): (_cross-build 'nucleus' board nucleus_link features) (_build-privileged board features) (_build-drivers board features) (_cross-build 'kickstart' board init_link features)
    @mkdir -p "{{ images_dir }}"
    {{ objcopy }} --strip-all -O binary "{{ target_root / config / target / image_profile / 'kickstart' }}" "{{ images_dir / 'kernel-' + config + '.bin' }}"
    @# TODO: print final binary size!
    @echo "{{ok_label}} kernel built for {{ board }}{{ if features != '' { ' [' + features + ']' } else { '' } }}: {{ images_dir / 'kernel-' + config + '.bin' }}"

# Build the EL0 userspace components (linked with the userspace runtime's user.ld) for the
# image configuration `board` + `image_features`
_build-components board='rpi3' features='qemu' image_features=features: (_cross-build user_components board user_link features '' image_features)

# === E2E test kernels (Kickstart images with test programs) ===

# Build one Kickstart-based e2e test kernel: the nucleus, the privileged
# components, the drivers Kickstart bundles, then `crate` linked as the init
# image, then its raw binary. Kernels bundling further userspace components
# build them first (_build-endpoint-test …).
_build-test-kernel crate board='rpi3' features=test_kernel_features config=(board + (if features == '' { '' } else { '-' + replace(features, ',', '+') }) + profile_suffix): (_cross-build 'nucleus' board nucleus_link features) (_build-privileged board features) (_build-drivers board features) (_cross-build crate board init_link features)
    @mkdir -p "{{ images_dir }}"
    {{ objcopy }} --strip-all -O binary "{{ target_root / config / target / image_profile / crate }}" "{{ images_dir / crate + '-' + config + '.bin' }}"
    @echo "{{ok_label}} {{ crate }} built for {{ board }} [{{ features }}]"

# Rebuild `crate` unconditionally with the `build` recipe line and boot it;
# in-guest assertions and the QEMU exit status are the result.
#
# The rebuild is deliberately a nested `just` invocation, not a recipe
# dependency: just deduplicates same-argument recipe dependencies within one
# invocation, so in `just ci` (clean lint build test) a plain dependency could
# be skipped as already run, leaving the kernel image stale. The nested
# invocation always runs and refreshes the image. `clock_opts` takes
# `qemu_icount_opts` for the tests that check timing bounds.
_run-test-kernel crate build=('_build-test-kernel ' + crate) board='rpi3' features=test_kernel_features clock_opts='' config=(board + (if features == '' { '' } else { '-' + replace(features, ',', '+') }) + profile_suffix):
    "{{ just_executable() }}" {{ build }}
    {{ qemu }} -M {{ if board == 'rpi3' { 'raspi3b' } else { 'raspi4b' } }} {{ qemu_io_opts }} {{ qemu_test_opts }} {{ clock_opts }} \
      -dtb "{{ if board == 'rpi3' { rpi3_dtb } else { rpi4_dtb } }}" \
      -kernel "{{ images_dir / crate + '-' + config + '.bin' }}"

# Build the endpoint-test e2e kernel (three-party rendezvous through an endpoint component)
_build-endpoint-test board='rpi3' features='qemu': (_build-components board features) (_build-test-kernel 'endpoint-test' board features)

# Build the fp-trap-test negative e2e kernel (its nucleus carries the test-only `fp_trap_test` trap hook)
_build-fp-trap-test board='rpi3': (_build-components board 'qemu' 'qemu,fp_trap_test') (_build-test-kernel 'fp-trap-test' board 'qemu,fp_trap_test')

# Build the preempt-test e2e kernel (timer tick and preemption of EL1t and EL0 Threads)
_build-preempt-test board='rpi3': (_build-components board 'qemu' test_kernel_features) (_build-test-kernel 'preempt-test' board)

# Build the dispatch-test e2e kernel (a userspace root dispatcher bound to the Scheduler)
_build-dispatch-test board='rpi3': (_build-components board 'qemu' test_kernel_features) (_build-test-kernel 'dispatch-test' board)

# Build the driver-test e2e kernel (Kickstart driver matching and the driver init page)
_build-driver-test board='rpi3': (_build-components board 'qemu' test_kernel_features) (_build-test-kernel 'driver-test' board)

# Build the mailbox-test e2e kernel (the mailbox component and a client wired through the firmware node)
_build-mailbox-test board='rpi3': (_build-components board 'qemu' test_kernel_features) (_build-test-kernel 'mailbox-test' board)

# Build the irq-test e2e kernel (interrupt delivery to an EL0 receiver Thread)
_build-irq-test board='rpi3': (_build-components board 'qemu' test_kernel_features) (_build-test-kernel 'irq-test' board)

# Build the uart-test e2e kernel (the PL011 driver component serving a client's IO channels in loopback)
_build-uart-test board='rpi3': (_build-components board 'qemu' test_kernel_features) (_build-test-kernel 'uart-test' board)

# Build the fault-test e2e kernel (fault delivery to EL0 fault handlers)
_build-fault-test board='rpi3': (_build-components board 'qemu') (_build-test-kernel 'fault-test' board 'qemu')

# Build the usb-test e2e kernel (the DWC2 host controller component, loaded by Kickstart)
_build-usb-test board='rpi3': (_build-components board 'qemu' test_kernel_features) (_build-test-kernel 'usb-test' board)

# Build the usb-core-test e2e kernel (the DWC2 component, the PL011 driver and the USB core, loaded by Kickstart)
_build-usb-core-test board='rpi3': (_build-components board 'qemu' test_kernel_features) (_build-test-kernel 'usb-core-test' board)

# Build the usb-hid-test e2e kernel (the DWC2 component, the PL011 driver, the USB core, the HID component and the input-trace program)
_build-usb-hid-test board='rpi3': (_build-components board 'qemu' test_kernel_features) (_build-test-kernel 'usb-hid-test' board)

# Build the sd-test e2e kernel (the SDHOST component and a program reading and writing the card through block_v1)
_build-sd-test board='rpi3': (_build-components board 'qemu' test_kernel_features) (_build-test-kernel 'sd-test' board)

# Build the compositor-test e2e kernel (the DWC2 component, the PL011 driver, the USB core, the HID component and the compositor)
_build-compositor-test board='rpi3': (_build-components board 'qemu' test_kernel_features) (_build-test-kernel 'compositor-test' board)

# Build the window-events-test e2e kernel: compositor-test's components plus window-events, a client checking the events of its two windows
_build-window-events-test board='rpi3': (_build-components board 'qemu' test_kernel_features) (_build-test-kernel 'window-events-test' board)

# Build log-test's programs: the probe and the log server, for the image configuration `board` + `image_features`; only log-test bundles the probe, so it is not in `user_components`
_build-log-probe board='rpi3' features='qemu' image_features=features: (_cross-build 'log-probe log-server' board user_link features '' image_features)

# Build the log-test e2e kernel: the PL011 driver, the log server and a probe whose Threads overflow a stack and panic
_build-log-test board='rpi3': (_build-components board 'qemu' test_kernel_features) (_build-log-probe board 'qemu' test_kernel_features) (_build-test-kernel 'log-test' board)

# Build the audio-test e2e kernel (the clock manager component, the DMA controller component, the PWM audio driver, the gpio component and the PL011 driver)
_build-audio-test board='rpi3': (_build-components board 'qemu' test_kernel_features) (_build-test-kernel 'audio-test' board)

# Remake targets/audio-test.dtb after changing the overlay or the rpi3 tree: the rpi3 tree with targets/overlays/audio-test-pwm.dts applied, which enables the PWM node with the jack's pins as the board's `dtoverlay=pwm-2chan,pin=40,func=4,pin2=41,func2=4` does (needs dtc and fdtoverlay)
[group("tools")]
audio-test-dtb:
    @mkdir -p "{{ target_root }}"
    dtc -q -@ -I dts -O dtb -o "{{ target_root / 'audio-test-pwm.dtbo' }}" "{{ justfile_directory() / 'targets/overlays/audio-test-pwm.dts' }}"
    fdtoverlay -i "{{ rpi3_dtb }}" -o "{{ audio_test_dtb }}" "{{ target_root / 'audio-test-pwm.dtbo' }}"

# === Hardware test images (booted on a Raspberry Pi 3 via chainofcommand) ===

# Build usb-core-test for the Raspberry Pi 3 hardware (no QEMU, no semihosting): `usb-core` traces on the UART
_build-usb-core-test-hw board='rpi3': (_build-components board '' '') (_build-test-kernel 'usb-core-test' board '')

# Build usb-core-test for the Raspberry Pi 3 and boot it via chainofcommand: `usb-core`'s trace appears on the UART
_boot-usb-core-test: _build-usb-core-test-hw chainofcommand
    target/debug/chainofcommand {{ chainofcommand_args }} --kernel "{{ images_dir / 'usb-core-test-rpi3.bin' }}"

# Build usb-hid-test for the Raspberry Pi 3 hardware (no QEMU, no semihosting): `input-trace` prints the keyboard's events on the UART
_build-usb-hid-test-hw board='rpi3': (_build-components board '' '') (_build-test-kernel 'usb-hid-test' board '')

# Build usb-hid-test for the Raspberry Pi 3 and boot it via chainofcommand: plug in a keyboard and press keys, `input-trace` prints the events on the UART
_boot-usb-hid-test: _build-usb-hid-test-hw chainofcommand
    target/debug/chainofcommand {{ chainofcommand_args }} --kernel "{{ images_dir / 'usb-hid-test-rpi3.bin' }}"

# Run the usb-hid-test hardware image in QEMU with the host's USB keyboard and mouse passed through (needs root)
_test-usb-hid-hw: _build-usb-hid-test-hw
    sudo {{ qemu }} -M raspi3b {{ qemu_io_opts }} {{ qemu_test_opts }} {{ qemu_usb_forward }} \
      -dtb "{{ rpi3_dtb }}" -kernel "{{ images_dir / 'usb-hid-test-rpi3.bin' }}"

# Build compositor-test for the Raspberry Pi 3 hardware (no QEMU, no semihosting): the compositor traces the routed input on the UART
_build-compositor-test-hw board='rpi3': (_build-components board '' '') (_build-test-kernel 'compositor-test' board '')

# Build compositor-test for the Raspberry Pi 3 and boot it via chainofcommand: plug in a mouse and a keyboard, the compositor traces the routed input on the UART
_boot-compositor-test: _build-compositor-test-hw chainofcommand
    target/debug/chainofcommand {{ chainofcommand_args }} --kernel "{{ images_dir / 'compositor-test-rpi3.bin' }}"

# Build audio-test for the Raspberry Pi 3 hardware (no QEMU, no semihosting): the test client streams a tone through audio_v1 to the audio driver's DMA ring, which traces each stream on the UART
_build-audio-test-hw board='rpi3': (_build-components board '' '') (_build-test-kernel 'audio-test' board '')

# Build audio-test for the Raspberry Pi 3 and boot it via chainofcommand: with `dtoverlay=pwm-2chan,pin=40,func=4,pin2=41,func2=4` in config.txt, the test client plays a 440 Hz tone through audio_v1 on the left side of the headphone jack for 3 s, the right for 3 s, both for 3 s, then both for a minute, 0.7 s apart; the audio driver traces on the UART, at each close, the measured sample rate, the underruns, late periods, the PWM's STA flags and the DMA errors
_boot-audio-test: _build-audio-test-hw chainofcommand
    target/debug/chainofcommand {{ chainofcommand_args }} --kernel "{{ images_dir / 'audio-test-rpi3.bin' }}"

# === Chainboot ===

# Build chainboot bootloader (features: '' for hw, 'qemu' for emulation) into target/images/chainboot-<config>.bin
[group("build")]
build-chainboot board='rpi4' features='': (_build-chainboot board features)

_build-chainboot board features config=(board + (if features == '' { '' } else { '-' + replace(features, ',', '+') }) + profile_suffix): (_cross-build 'chainboot' board chainboot_link features)
    @mkdir -p "{{ images_dir }}"
    {{ objcopy }} --strip-all -O binary "{{ target_root / config / target / image_profile / 'chainboot' }}" "{{ images_dir / 'chainboot-' + config + '.bin' }}"
    @just _audit-chainboot-exclusives "{{ target_root / config / target / image_profile / 'chainboot' }}"
    @echo "{{ok_label}} chainboot built for {{ board }}{{ if features != '' { ' [' + features + ']' } else { '' } }}: {{ images_dir / 'chainboot-' + config + '.bin' }}"

# Fail if the chainboot ELF holds an exclusive or atomic read-modify-write instruction: chainboot runs without the data cache, where the Pi's cores never complete them and the program hangs without a sound. QEMU completes them, so only this check (and the board) can tell.
_audit-chainboot-exclusives elf:
    #!/usr/bin/env bash
    set -euo pipefail
    # Load/store-exclusive, and the LSE atomics: add, clear, xor, set, min and max loads and stores, swap, compare-and-swap.
    # The address column is padded to the widest address, so a full-width one starts at the margin.
    exclusive='^[[:space:]]*[0-9a-f]+:[[:space:]]+(ldx(r[bh]?|p)|ldaxr[bh]?|ldaxp|stx(r[bh]?|p)|stlxr[bh]?|stlxp|(ld|st)(add|clr|eor|set|smax|smin|umax|umin)(a|l|al)?[bh]?|swp(a|l|al)?[bh]?|cas(p)?(a|l|al)?[bh]?)[[:space:]]'
    found=$(rust-objdump -d --no-show-raw-insn "{{ elf }}" | sed 's/<[^>]*>//g' | grep -E "${exclusive}" || true)
    if [ -n "${found}" ]; then
        echo "❌ {{ file_name(elf) }} contains exclusive or atomic read-modify-write instructions:"
        echo "${found}"
        exit 1
    fi
    echo "{{ ok_label }} {{ file_name(elf) }}: no exclusive or atomic read-modify-write instructions"

# === Host tools ===

# Build chainofcommand serial loader
[group("build")]
chainofcommand:
    @cargo build -p chainofcommand
    @echo "Run 'just boot' to boot via coc"

alias coc := chainofcommand

# Build the Meddle interface compiler (target/debug/meddler)
[group("tools")]
meddler:
    @cargo build -p meddler

# === QEMU runners ===

# Build and run kernel in QEMU
[group("emu")]
qemu: (build 'rpi3' 'qemu')
    @echo "🚜 Run QEMU {{ qemu_base_opts }} with {{ kernel_rpi3_qemu_bin }}"
    @echo "🚜 .. on {{ rpi3_dtb }}"
    @rm -f qemu.log
    {{ qemu }} {{ qemu_base_opts }} -dtb "{{ rpi3_dtb }}" -kernel "{{ kernel_rpi3_qemu_bin }}"

# Build and run kernel in QEMU with GDB port
[group("emu")]
qemu-gdb: (build 'rpi3' 'qemu')
    @echo "🚜 Run QEMU {{ qemu_base_opts }} {{ qemu_disasm_gdb }} with {{ kernel_rpi3_qemu_bin }}"
    @echo "🚜 .. on {{ rpi3_dtb }}"
    @rm -f qemu.log
    {{ qemu }} {{ qemu_base_opts }} {{ qemu_disasm_gdb }} -dtb "{{ rpi3_dtb }}" -kernel "{{ kernel_rpi3_qemu_bin }}"

# Build and run chainboot in QEMU
[group("emu")]
cb-qemu: (build-chainboot 'rpi3' 'qemu')
    @echo "🚜 Run QEMU {{ qemu_base_opts }} {{ qemu_disasm }} with {{ chainboot_rpi3_qemu_bin }}"
    @echo "🚜 .. on {{ rpi3_dtb }}"
    @rm -f qemu.log
    {{ qemu }} -serial tcp:127.0.0.1:4321,server,nowait {{ qemu_base_opts }} -dtb "{{ rpi3_dtb }}" -kernel "{{ chainboot_rpi3_qemu_bin }}"

# Build and run chainboot in QEMU with GDB port
[group("emu")]
cb-qemu-gdb: (build-chainboot 'rpi3' 'qemu')
    @echo "🚜 Run QEMU {{ qemu_base_opts }} {{ qemu_disasm_gdb }} with {{ chainboot_rpi3_qemu_bin }}"
    @echo "🚜 .. on {{ rpi3_dtb }}"
    @rm -f qemu.log
    {{ qemu }} {{ qemu_base_opts }} {{ qemu_disasm_gdb }} -serial pty -dtb "{{ rpi3_dtb }}" -kernel "{{ chainboot_rpi3_qemu_bin }}"

# Build and boot in qemu via chainofcommand
[group("emu")]
boot-qemu: (build 'rpi3' 'qemu') chainofcommand
    target/debug/chainofcommand {{ chainofcommand_args }} --kernel "{{ kernel_rpi3_qemu_bin }}"

# === Zellij (QEMU in split terminal) ===

# The zellij runs name their serial ports themselves (the PL011, then the mini UART, each
# on its own pane), so only the monitor and semihosting stay on stdio
_write-zellij-config bin runner_opts dtb machine=qemu_machine:
    #!/usr/bin/env bash
    cat > emulation/zellij-config.sh <<EOF
    QEMU="{{ qemu }}"
    QEMU_OPTS="-M {{ machine }} {{ qemu_stdio_opts }} {{ if machine == 'raspi3b' { qemu_usb } else { '' } }}"
    QEMU_RUNNER_OPTS="{{ runner_opts }}"
    CARGO_MAKE_WORKSPACE_WORKING_DIRECTORY="{{ justfile_directory() }}"
    TARGET_DTB="{{ dtb }}"
    KERNEL_BIN="{{ bin }}"
    EOF

# Build and run kernel in QEMU with PL011 and the mini UART each in a pane
[group("emu")]
zellij: (build 'rpi3' 'qemu') (_write-zellij-config kernel_rpi3_qemu_bin '' rpi3_dtb)
    zellij --layout emulation/layout.zellij

alias z-qemu := zellij

# Like zellij, but QEMU waits for GDB on port 5555 and traces instructions
[group("emu")]
zellij-gdb: (build 'rpi3' 'qemu') (_write-zellij-config kernel_rpi3_qemu_bin qemu_disasm_gdb rpi3_dtb)
    zellij --layout emulation/layout.zellij

# Like zellij, for the Raspberry Pi 4 kernel on QEMU's raspi4b machine
[group("emu")]
zellij-rpi4: (build 'rpi4' 'qemu') (_write-zellij-config kernel_rpi4_qemu_bin '' rpi4_dtb 'raspi4b')
    zellij --layout emulation/layout.zellij

# Build and run chainboot in QEMU with serial port emulation
[group("emu")]
cb-zellij: (build-chainboot 'rpi3' 'qemu') (_write-zellij-config chainboot_rpi3_qemu_bin qemu_disasm rpi3_dtb)
    zellij --layout emulation/layout.zellij

# Run chainboot with GDB in zellij window
[group("emu")]
cb-zellij-gdb: (build-chainboot 'rpi3' 'qemu') (_write-zellij-config chainboot_rpi3_qemu_bin qemu_disasm_gdb rpi3_dtb)
    zellij --layout emulation/layout.zellij

# === GDB and disassembly ===

_write-gdb-config:
    #!/usr/bin/env bash
    mkdir -p "$(dirname "{{ gdb_connect }}")"
    cat > "{{ gdb_connect }}" <<EOF
    target extended-remote :5555
    break *0x80000
    break main
    break kickstart_run
    break cap_invoke_handler
    EOF
    echo "🖌️ Generated GDB config file {{ gdb_connect }}"

# Build and run kernel in GDB (connect to openocd or QEMU on port 5555)
[group("debug")]
gdb: build _write-gdb-config
    @pipx run gdbgui -g "{{ gdb }} -x '{{ gdb_connect }}' '{{ kernel_rpi4_elf }}'"

# Build and run chainboot in GDB
[group("debug")]
cb-gdb: build-chainboot _write-gdb-config
    {{ gdb }} -x "{{ gdb_connect }}" "{{ chainboot_rpi4_elf }}"

# Build and disassemble kernel
[group("debug")]
hopper: build
    hopper --loader ELF --executable "{{ kernel_rpi4_elf }}"

alias disasm := hopper

# Build and disassemble chainboot
[group("debug")]
cb-hopper: (build-chainboot 'rpi3' 'qemu')
    #hopper --loader RAW --plugin arm --cpu aarch64 --variant generic --base-address 0x80000 --executable "{{ chainboot_rpi3_qemu_bin }}"

alias cb-disasm := cb-hopper

# Build and print all symbols
[group("maintenance")]
nm: build
    {{ nm }} "{{ kernel_rpi4_elf }}" | sort -k 1 | rustfilt

# Run `cargo expand` on kernel
[group("maintenance")]
expand:
    cargo expand {{ target_json }} --release -- kernel

# === Hardware: SD card, serial boot, JTAG ===

# Build and write kernel to SD Card
[group("hw")]
device: build
    cp "{{ kernel_rpi4_bin }}" "{{ volume }}/kernel8.img"
    @echo "{{copy_label}} copied kernel to {{ volume }}/kernel8.img"

# Build and write kernel to SD Card, then eject
[group("hw")]
device-eject: device
    diskutil ejectAll "{{ volume }}"

# Build and write chainboot to SD Card, then eject
[group("hw")]
cb-eject board='rpi4': (build-chainboot board)
    cp "{{ images_dir / 'chainboot-' + board + '.bin' }}" "{{ volume }}/{{ 'chainboot-' + board + '.bin' }}"
    @echo "{{copy_label}} copied chainboot to {{ volume }}/{{ 'chainboot-' + board + '.bin' }}"
    # TODO: sed proper kernel name here in config.txt
    diskutil unmountDisk "{{ volume }}"

# Build and boot via chainofcommand
[group("hw")]
boot: build chainofcommand
    target/debug/chainofcommand {{ chainofcommand_args }} --kernel "{{ kernel_rpi4_bin }}"

# Start this, then power the board on: Kickstart offers a faster console for 1.5 s at boot and
# chainofcommand asks for `chainboot_load_baud`; it sends no kernel when chainboot asks for one.
# Be the serial console of a kernel booted from the SD card, without chainboot
[group("hw")]
console: chainofcommand
    target/debug/chainofcommand {{ chainofcommand_args }} --console

# Start openocd connected via JTAG
[group("hw")]
openocd board='rpi4':
    {{ openocd_bin }} -f interface/jlink.cfg -f ../ocd/{{ board }}_target.cfg

alias ocd := openocd

# === Testing ===

# The device, nucleus and e2e kernel tests in QEMU (rpi3, some also rpi4), chainboot's, and the host tests.
# Run every test
[group("test")]
test: _test-device _test-chainboot test-host _test-debug-console _test-key-table _test-untyped _test-capability _test-memory _test-sync _test-ppc _test-preempt _test-preempt-rpi4 _test-dispatch _test-dispatch-rpi4 _test-irq _test-irq-rpi4 _test-driver _test-driver-rpi4 _test-mailbox _test-mailbox-rpi4 _test-uart _test-uart-rpi4 _test-endpoint _test-fp-trap _test-fault _test-usb _test-usb-core _test-usb-hid _test-compositor _test-sd _test-audio _test-dma _test-log

alias t := test

# Run device crate tests in QEMU (rpi3) --verbose
_test-device: (_cross-build 'nucleus' 'rpi3' nucleus_link 'qemu') (_build-privileged 'rpi3' 'qemu') (_build-drivers 'rpi3' 'qemu')
    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ device_test_rustflags }}" \
    cargo test --tests {{ target_json }} --features=qemu {{ rust_std }} \
      --workspace --exclude=chainofcommand --exclude=vesper-image-build --exclude=metta-meddle --exclude=meddler --exclude=chainboot

    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ device_test_rustflags }}" \
    cargo test --doc {{ target_json }} --features=qemu {{ rust_std }} \
    --workspace --exclude=chainofcommand --exclude=vesper-image-build --exclude=metta-meddle --exclude=meddler --exclude=chainboot --exclude=vesper-boot-uart

# Run one nucleus integration test binary in QEMU (rpi3)
_test-nucleus test features='qemu':
    RUSTFLAGS="{{ device_test_rustflags }}" \
    cargo test -p nucleus --test {{ test }} {{ target_json }} \
      --features={{ features }} {{ rust_std }}

# Test the debug-only nucleus console handler and the scheduler/PPC suites in QEMU (rpi3)
_test-debug-console: (_test-nucleus 'debug_console' 'qemu,debug_kernel')

# Test the nucleus KeyTable management handler in QEMU (rpi3)
_test-key-table: (_test-nucleus 'key_table' 'qemu,debug_kernel')

# Test the nucleus Untyped Retype handler in QEMU (rpi3)
_test-untyped: (_test-nucleus 'untyped')

# Boot capability-test: boot-table invariants, debug console key, KeyTable/Frame Retype, Untyped split
_test-capability: (_run-test-kernel 'capability-test')

# Boot ppc-test: Invocation construction and same-Thread PPC Call/Return into the Bounce AddressSpace
_test-ppc: (_run-test-kernel 'ppc-test')

# Boot sync-test: Notification, EventCount, blocking waits through the Bounce Thread, Thread.Retire
_test-sync: (_run-test-kernel 'sync-test')

# Boot memory-test: PageTable/Frame mapping, alias policy, ASIDs, activation and TLB invalidation, AddressSpace.Retire
_test-memory: (_run-test-kernel 'memory-test')

# Boot preempt-test: interrupt-controller component, kernel timer tick and round-robin preemption of Threads that never yield
_test-preempt: (_run-test-kernel 'preempt-test' '_build-preempt-test' 'rpi3' test_kernel_features qemu_icount_opts)

# Boot preempt-test on the Raspberry Pi 4 machine: the same image, ticking through the GICv2 component
_test-preempt-rpi4: (_run-test-kernel 'preempt-test' '_build-preempt-test rpi4' 'rpi4' test_kernel_features qemu_icount_opts)

# Boot dispatch-test: a userspace root dispatcher runs worker Threads round-robin through Scheduler.Dispatch
_test-dispatch: (_run-test-kernel 'dispatch-test' '_build-dispatch-test' 'rpi3' test_kernel_features qemu_icount_opts)

# Boot dispatch-test on the Raspberry Pi 4 machine
_test-dispatch-rpi4: (_run-test-kernel 'dispatch-test' '_build-dispatch-test rpi4' 'rpi4' test_kernel_features qemu_icount_opts)

# Boot driver-test: Kickstart loads a probe driver per matching device tree node and hands it registers, interrupts, memory and a Time; the PL011 probe's high-priority Thread must wake on time beside two spinning normal Threads (the root dispatcher's priorities), and a spinning input Thread must not keep them off the CPU (the starvation guard)
_test-driver: (_run-test-kernel 'driver-test' '_build-driver-test' 'rpi3' test_kernel_features qemu_icount_opts)

# Boot driver-test on the Raspberry Pi 4 machine: the same checks through the GICv2 component
_test-driver-rpi4: (_run-test-kernel 'driver-test' '_build-driver-test rpi4' 'rpi4' test_kernel_features qemu_icount_opts)

# Boot mailbox-test: Kickstart loads the mailbox component (through the kernel-owned page on the Raspberry Pi 3) and a client on the firmware power node that calls it
_test-mailbox: (_run-test-kernel 'mailbox-test' '_build-mailbox-test')

# Boot mailbox-test on the Raspberry Pi 4 machine: the mailbox page is an ordinary device page there
_test-mailbox-rpi4: (_run-test-kernel 'mailbox-test' '_build-mailbox-test rpi4' 'rpi4')

# Boot uart-test: the PL011 driver component's two Threads serve a client's transmit and receive IO channels in loopback by interrupts
_test-uart: (_run-test-kernel 'uart-test' '_build-uart-test')

# Boot uart-test on the Raspberry Pi 4 machine: the same exchange through the GICv2 component
_test-uart-rpi4: (_run-test-kernel 'uart-test' '_build-uart-test rpi4' 'rpi4')

# Boot irq-test: IRQControl.Issue, IRQHandler binding and Ack, repeated virtual-timer interrupts delivered to an EL0 receiver Thread
_test-irq: (_run-test-kernel 'irq-test' '_build-irq-test')

# Boot irq-test on the Raspberry Pi 4 machine: the same checks through the GICv2 component
_test-irq-rpi4: (_run-test-kernel 'irq-test' '_build-irq-test rpi4' 'rpi4')

# Boot endpoint-test: client, endpoint and server AddressSpaces rendezvous through PPC
_test-endpoint: (_run-test-kernel 'endpoint-test' '_build-endpoint-test' 'rpi3' 'qemu')

# Boot fp-trap-test: an FP/SIMD instruction must trap at EL1t and at EL0
_test-fp-trap: (_run-test-kernel 'fp-trap-test' '_build-fp-trap-test' 'rpi3' 'qemu,fp_trap_test')

# Boot fault-test: faults are delivered to EL0 fault handlers (skip, retry, terminate, Return faults) and every unhandled case parks the Thread
_test-fault: (_run-test-kernel 'fault-test' '_build-fault-test' 'rpi3' 'qemu')

# Boot usb-test on raspi3b with a hub on the root port: the DWC2 component resets the port and enumerates the hub
_test-usb board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (_build-usb-test board)
    {{ qemu }} -M raspi3b {{ qemu_io_opts }} {{ qemu_test_opts }} {{ qemu_usb }} \
      -dtb "{{ rpi3_dtb }}" -kernel "{{ images_dir / 'usb-test-' + config + '.bin' }}"

# Boot usb-core-test on raspi3b with a hub on the root port and a keyboard behind it: the USB core enumerates both
_test-usb-core board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (_build-usb-core-test board)
    {{ qemu }} -M raspi3b {{ qemu_io_opts }} {{ qemu_test_opts }} {{ qemu_usb }} \
      -dtb "{{ rpi3_dtb }}" -kernel "{{ images_dir / 'usb-core-test-' + config + '.bin' }}"

# Boot usb-hid-test on raspi3b with a hub on the root port and a keyboard behind it: the HID component claims the keyboard and input-trace reads it through input_v1
_test-usb-hid board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (_build-usb-hid-test board)
    {{ qemu }} -M raspi3b {{ qemu_io_opts }} {{ qemu_test_opts }} {{ qemu_usb }} \
      -dtb "{{ rpi3_dtb }}" -kernel "{{ images_dir / 'usb-hid-test-' + config + '.bin' }}"

# Boot sd-test on raspi3b with a fresh 4 MiB card image whose blocks 0, 5, 6 and 8191 start with a marker: the program reads them, writes a block back and checks the request rules
_test-sd board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (_build-sd-test board)
    #!/usr/bin/env bash
    set -euo pipefail
    image="{{ target_root / 'sd-test.img' }}"
    dd if=/dev/zero of="${image}" bs=1048576 count=4 2>/dev/null
    for block in 0 5 6 8191; do
        printf 'vesper sd block %d' "${block}" | dd of="${image}" bs=512 seek="${block}" conv=notrunc 2>/dev/null
    done
    {{ qemu }} -M raspi3b {{ qemu_io_opts }} {{ qemu_test_opts }} -drive if=sd,format=raw,file="${image}" \
      -dtb "{{ rpi3_dtb }}" -kernel "{{ images_dir / 'sd-test-' + config + '.bin' }}"

# Boot audio-test on raspi3b with the audio test tree: the audio driver sets the PWM clock to 100 MHz through the clock manager component and reads it back, and clock_v1 refuses an unknown clock and an unreachable rate; then the test client runs the capture test through audio_v1: QEMU has no PWM block and cannot run a DMA ring, so the driver plays each period by a DMA copy into a capture buffer and checks the captured words against the client's pattern (converted, in order, silence and underruns once the client stops), and packets come back in order (`just _boot-audio-test` plays a tone on the board). The console goes to target/audio-test.log
_test-audio board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (_build-audio-test board)
    #!/usr/bin/env bash
    set -uo pipefail
    log="{{ target_root }}/audio-test.log"
    rm -f "$log"
    {{ qemu }} -M raspi3b -display none -monitor none \
      -chardev stdio,mux=on,id=char0,signal=off -serial chardev:char0 -semihosting-config enable=on,userspace=on,chardev=char0 \
      -dtb "{{ audio_test_dtb }}" -kernel "{{ images_dir / 'audio-test-' + config + '.bin' }}" < /dev/null > "$log"
    status=$?
    grep -a "cprman: \|audio: \|audio-test: " "$log"
    [ "$status" -eq 0 ] || { echo "audio-test: failed with status $status; see $log"; exit 1; }
    expect() { grep -q "$1" "$log" || { echo "audio-test: $2; see $log"; exit 1; }; }
    expect "audio: PWM clock set to 100000000 Hz, reads back 100000000 Hz" "the PWM clock did not read back 100 MHz"
    expect "audio: clock_v1 refused clock 31 and 48000000 Hz" "clock_v1 did not refuse"
    expect "audio-test: 7 packets back in order, 3072 frames taken, the refused one empty" "the first stream's packets did not come back in order"
    expect "audio: capture checked: 1 ramp up, [0-9]* stream periods with 3072 client frames of the pattern in order, [1-9][0-9]* of them silent at the end, 1 ramp down, [0-9]* off; DMA errors 0x0" "the captured words of the first stream are not the client's frames converted"
    expect "audio: closed after [0-9]* periods: 3072 client frames, [1-9][0-9]* underruns" "no underruns were counted after the client stopped"
    expect "audio-test: the client stopped sending: [1-9][0-9]* underruns counted, closed" "the client did not see the underruns"
    expect "audio: capture checked: 1 ramp up, 4 stream periods with 2048 client frames of the pattern in order, 0 of them silent at the end, 1 ramp down" "close did not play out the queued frames"
    expect "audio: closed after 6 periods: 2048 client frames, 0 underruns" "the end of a stream counted as an underrun"
    expect "audio-test: closed at once: 4 packets back in order, 2048 frames played out, no underruns" "the packets queued at close did not come back whole"
    echo "{{ ok_label }} the PWM clock runs at 100 MHz from PLLD through clock_v1; PCM streams through audio_v1 into the capture buffer converted and in order, with underruns counted"

# Boot audio-test on raspi3b for its DMA checks: Kickstart hands the DMA controller component each interrupt line once, the audio driver copies memory through dma_v1 with chains of linked control blocks on the first channel and on one sharing line 27, one completion per block, and dma_v1 refuses what it must. QEMU's DMA model runs a chain inside the write that starts it, so no ring runs here (`just _boot-audio-test` plays one on the board). The console goes to target/dma-test.log
_test-dma board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (_build-audio-test board)
    #!/usr/bin/env bash
    set -uo pipefail
    log="{{ target_root }}/dma-test.log"
    rm -f "$log"
    {{ qemu }} -M raspi3b -display none -monitor none \
      -chardev stdio,mux=on,id=char0,signal=off -serial chardev:char0 -semihosting-config enable=on,userspace=on,chardev=char0 \
      -dtb "{{ audio_test_dtb }}" -kernel "{{ images_dir / 'audio-test-' + config + '.bin' }}" < /dev/null > "$log"
    status=$?
    grep -a "dma: \|audio: DMA\|audio: dma_v1\|audio-test: ✅" "$log"
    [ "$status" -eq 0 ] || { echo "dma-test: failed with status $status; see $log"; exit 1; }
    expect() { grep -q "$1" "$log" || { echo "dma-test: $2; see $log"; exit 1; }; }
    expect "dma: channels 0x7f35 granted, 13 interrupts," "the DMA controller did not get its mask and 13 interrupt lines"
    expect "dma: channel 0 interrupt: INT_STATUS 0x0001, END true, INT true, ERROR false, 4 blocks finished, chain ended" "channel 0 did not interrupt with END set after 4 blocks"
    expect "audio: DMA copied 2048 bytes in 4 linked blocks on channel 0: 4 completions, destination matches" "the 4-block copy did not arrive"
    expect "dma: channel 14 interrupt: INT_STATUS 0x4000, END true, INT true, ERROR false, 2 blocks finished, chain ended" "channel 14 did not interrupt on the shared line"
    expect "audio: DMA copied 1024 bytes in 2 blocks on channel 14, on the shared interrupt line" "the copy on the shared line did not arrive"
    expect "audio: dma_v1 refused" "dma_v1 did not refuse"
    echo "{{ ok_label }} DMA chains copy memory, one completion per control block, on own and shared interrupt lines"

# Boot compositor-test on raspi3b with a hub on the root port and a keyboard behind it: the compositor reads the keyboard through input_v1 and has drawn the background on the framebuffer
_test-compositor board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (_build-compositor-test board)
    @rm -f target/compositor-test.log
    {{ qemu }} -M raspi3b {{ qemu_io_opts }} {{ qemu_test_opts }} -device usb-hub,port=1 -device usb-kbd,port=1.1 \
      -dtb "{{ rpi3_dtb }}" -kernel "{{ images_dir / 'compositor-test-' + config + '.bin' }}" > target/compositor-test.log
    @grep -q "compositor: frame 1 drew the background: true" target/compositor-test.log || { echo "compositor-test: the background did not reach the framebuffer"; exit 1; }
    @echo "{{ ok_label }} the compositor drew the background and read the keyboard"

# Boot window-events-test on raspi3b with a USB keyboard and mouse behind a hub: once window-events has made its windows (B, the newest, has the focus), click on A, drag over B, release and type `a` through the QEMU monitor; window-events checks what each window received and ends the run with the result. The console goes to target/window-events-test.log
_test-window-events board='rpi3' seconds='120' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (_build-window-events-test board)
    #!/usr/bin/env bash
    set -uo pipefail
    log="{{ target_root }}/window-events-test.log"
    monitor="{{ target_root }}/window-events-test-monitor.sock"
    rm -f "$log" "$monitor"
    {{ qemu }} -M raspi3b -display none -monitor "unix:$monitor,server,nowait" \
      -chardev stdio,mux=on,id=char0,signal=off -serial chardev:char0 -semihosting-config enable=on,userspace=on,chardev=char0 \
      -device usb-hub,port=1 -device usb-kbd,port=1.1 -device usb-mouse,port=1.2 \
      -dtb "{{ rpi3_dtb }}" -kernel "{{ images_dir / 'window-events-test-' + config + '.bin' }}" < /dev/null > "$log" &
    qemu_pid=$!
    monitor_command() { printf '%s\n' "$1" | socat - "UNIX-CONNECT:$monitor" > /dev/null; }
    # The windows exist and the compositor has the keyboard and the mouse before the script runs.
    for _ in $(seq {{ seconds }}); do
      grep -q "window-events: windows" "$log" && grep -q "compositor: device 1:" "$log" && grep -q "compositor: device 2:" "$log" && break
      sleep 1
    done
    sleep 1
    # The pointer starts in the middle of the screen, inside A.
    monitor_command "mouse_button 1"
    sleep 1
    monitor_command "mouse_move 300 0"
    sleep 1
    monitor_command "mouse_button 0"
    sleep 1
    monitor_command "sendkey a"
    for _ in $(seq 60); do
      kill -0 "$qemu_pid" 2> /dev/null || break
      sleep 1
    done
    if kill -0 "$qemu_pid" 2> /dev/null; then
      monitor_command "quit"
      wait "$qemu_pid"
      echo "window-events-test: window-events did not finish; see $log"
      exit 1
    fi
    wait "$qemu_pid"
    status=$?
    grep -a "window-events: [✅❌]" "$log"
    [ "$status" -eq 0 ] || { echo "window-events-test: failed with status $status; see $log"; exit 1; }
    echo "{{ ok_label }} a new window took the focus, a click moved it, the drag stayed with the window pressed on and the key reached the focused window"

# Boot log-test: the probe's second Thread overflows its stack and its fault handler (liblog_client's, installed by Kickstart) logs `FAULT: log_probe thread 1: stack overflow (guard page)`; then its first Thread panics and the panic handler logs `PANICKED: log_probe: made to panic on purpose at …`, which ends the run. Both lines must reach the UART through the log server. The semihosting console goes to target/log-test.log, the UART to target/log-test-serial.log
_test-log board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (_build-log-test board)
    #!/usr/bin/env bash
    set -uo pipefail
    log="{{ target_root }}/log-test.log"
    serial="{{ target_root }}/log-test-serial.log"
    rm -f "$log" "$serial"
    # GNU timeout (`gtimeout` from Homebrew coreutils on macOS); without one the run is unbounded.
    limit=$(command -v timeout || command -v gtimeout || true)
    ${limit:+$limit 120} {{ qemu }} -M raspi3b -display none -monitor none \
      -chardev stdio,mux=on,id=char0,signal=off -serial "file:$serial" -semihosting-config enable=on,userspace=on,chardev=char0 \
      -dtb "{{ rpi3_dtb }}" -kernel "{{ images_dir / 'log-test-' + config + '.bin' }}" < /dev/null > "$log"
    status=$?
    grep -a "log-test: \|log-probe: \|FAULT: \|PANICKED" "$log" | tail -12
    cat "$serial"
    # The panic ends the run with a failure exit; a timeout (124) means it never came.
    [ "$status" -ne 124 ] || { echo "log-test: timed out; see $log"; exit 1; }
    grep -a -q "log-test: ✅ the probe has its fault handler" "$log" || { echo "log-test: the probe's fault handler was not installed; see $log"; exit 1; }
    # Only the file name and line of the panic: the path before them is the host's spelling.
    grep -a -q "EL0 PANICKED: panicked at .*main\.rs:[0-9]*" "$log" || { echo "log-test: no panic on the semihosting console; see $log"; exit 1; }
    prefix='^\[ *[0-9]*\.[0-9]\{6\}\] '
    for expected in "I log-server: 1 clients, serial output attached" "I log-probe: ready" "E FAULT: log_probe thread 1: stack overflow (guard page) at pc 0x[0-9a-f]*, far 0x[0-9a-f]*, sp 0x[0-9a-f]*, esr 0x[0-9a-f]*" "E PANICKED: log_probe: made to panic on purpose at .*main\.rs:[0-9]*"; do
      grep -a -q "${prefix}${expected}" "$serial" || { echo "log-test: the UART got no line \"$expected\" from the log server"; exit 1; }
    done
    echo "{{ ok_label }} the probe's stack overflow and panic reached the UART through the log server as FAULT: and PANICKED: lines"

# Boot the production image (rpi3, QEMU) and type commands at the shell on the serial console (the kernel's semihosting output goes to its own file), from a slow feeder so the UART's FIFO never overflows: the commands must answer, and `end` must shut QEMU down with a success status
_test-shell: (build 'rpi3' 'qemu')
    @rm -f qemu.log target/shell-test.log target/shell-semihosting.log
    (sleep 10; printf 'help\n'; sleep 2; printf 'led on\n'; sleep 2; printf 'led off\n'; sleep 2; printf 'feats\n'; sleep 2; printf 'draw\n'; sleep 2; printf 'end\n'; sleep 3) | {{ qemu }} -M raspi3b -display none -monitor none {{ qemu_usb }} -chardev stdio,id=uart,signal=off -serial chardev:uart -chardev file,id=semihosting,path=target/shell-semihosting.log -semihosting-config enable=on,userspace=on,chardev=semihosting -dtb "{{ rpi3_dtb }}" -kernel "{{ kernel_rpi3_qemu_bin }}" > target/shell-test.log
    @grep -q "Supported console commands" target/shell-test.log || { echo "shell-test: help printed nothing"; exit 1; }
    @grep -q "Unknown command feats, try 'help'" target/shell-test.log || { echo "shell-test: an unknown command was not reported"; exit 1; }
    @if grep -q "led on failed\|led off failed" target/shell-test.log; then echo "shell-test: the LED command failed"; exit 1; fi
    @grep -q "font: 95 glyphs mapped" target/shell-test.log || { echo "shell-test: the font resource was not mapped"; exit 1; }
    @if grep -q "draw failed" target/shell-test.log; then echo "shell-test: draw failed"; exit 1; fi
    @grep -q "lamp: frame 1, [1-9][0-9]* sampled cells lit" target/shell-semihosting.log || { echo "shell-test: the lamp drew nothing"; exit 1; }
    @grep -q "lamp: frame 20," target/shell-semihosting.log || { echo "shell-test: the lamp stopped animating"; exit 1; }
    @grep -q "compositor: frame 1 drew the background: true" target/shell-semihosting.log || { echo "shell-test: the compositor drew no background"; exit 1; }
    @grep -q "compositor: frame [0-9]*: 1 windows, [1-9][0-9]* sampled pixels differ from the background" target/shell-semihosting.log || { echo "shell-test: the shell's window did not reach the framebuffer"; exit 1; }
    @grep -q "Bye" target/shell-test.log || { echo "shell-test: end did not say goodbye"; exit 1; }
    @echo "{{ ok_label }} shell answered help, the LED commands, an unknown command and end, and drew in a window of the compositor"

# Run chainboot tests in QEMU (rpi3) with its own linker script
_test-chainboot:
    RUSTFLAGS="{{ fixed_rustflags }} {{ board_rpi3_flags }} -C link-arg=--script={{ chainboot_link }}" \
    cargo test {{ target_json }} --features=qemu {{ rust_std }} \
      -p chainboot

# Test runner invoked by .cargo/config.toml runner
_test-runner binary_path:
    #!/usr/bin/env bash
    set -euo pipefail
    name=$(basename "{{ binary_path }}")
    mkdir -p "{{ images_dir }}"
    bin="{{ images_dir }}/test-${name}.bin"
    {{ objcopy }} --strip-all -O binary "{{ binary_path }}" "${bin}"
    echo "🚨 Running test: ${name}"
    {{ qemu_test }} -kernel "${bin}"

# === Host tests ===

# The capability ABI, the boot platform, drivers' logic, Meddle, USB, input, compositor, dispatcher,
# log server, chainboot protocol and chainofcommand.
# Run the host-side tests natively
[group("test")]
test-host: _test-object-host _test-platform-host _test-meddle _test-usb-host _test-input-host _test-compositor-host _test-dispatcher-host _test-log-host _test-chainboot-host

# Run one opt-in (`host-tests`) integration test natively (currently AArch64 hosts)
_host-test package test features='host-tests':
    RUSTFLAGS="{{ fixed_rustflags }}" \
    cargo test -p {{ package }} --features={{ features }} --test {{ test }}

# Run the opt-in capability ABI tests on the native host
_test-object-host: (_host-test 'vesper-objects' 'object_type') (_host-test 'vesper-objects' 'object_type' 'host-tests,debug_kernel')

# Run the boot-platform tests natively: device tree helpers against the board
# DTBs in targets/, privileged-image loading, interrupt-controller register logic
_test-platform-host: (_host-test 'vesper-devicetree' 'devicetree') (_host-test 'vesper-image' 'privileged') (_host-test 'irqchip-bcm2836' 'bcm2836') (_host-test 'irqchip-gicv2' 'gicv2') (_host-test 'vesper-io-channel' 'io_channel') (_host-test 'vesper-serial-channel' 'serial_channel') (_host-test 'vesper-block-channel' 'block_channel') (_host-test 'uart-pl011' 'pl011') (_host-test 'sdhost-bcm2835' 'sdhost') (_host-test 'gpio-bcm2835' 'gpio') (_host-test 'vesper-device-driver' 'areas') (_host-test 'vesper-device-driver' 'masters') (_host-test 'mailbox-impl' 'tags') (_host-test 'power-impl' 'pm') (_host-test 'cprman-bcm2835' 'cprman') (_host-test 'dma-bcm2835' 'dma') (_host-test 'audio-pwm-bcm2835' 'pwm') (_host-test 'audio-pwm-bcm2835' 'stream') (_host-test 'fb-impl' 'requests') (_host-test 'shell' 'commands') (_host-test 'vesper-graphics' 'graphics')

# Run the Meddle interface compiler tests and the generated code's runtime tests natively
_test-meddle: (_host-test 'metta-meddle-runtime' 'words')
    cargo test -p metta-meddle

# Run the USB protocol logic tests natively: setup packets, descriptors, enumeration, HID boot reports
_test-usb-host: (_host-test 'vesper-usb-protocol' 'protocol') (_host-test 'usb-hcd-dwc2' 'dwc2') (_host-test 'usb-core' 'usb_core') (_host-test 'usb-hid' 'hid')

# Run the input event library tests natively: record layout, descriptor, state tracker
_test-input-host: (_host-test 'vesper-input-event' 'input_event')

# Run the compositor logic tests natively: windows, hit-testing, focus, grab and input routing
_test-compositor-host: (_host-test 'metta-compositor-core' 'compositor_core')

# Run the root dispatcher's host tests: the highest runnable priority first, round-robin within a priority, the starvation guard
_test-dispatcher-host: (_host-test 'root-dispatcher' 'policy')

# Run the log server's host tests: the line header, the client's buffer set, line cutting and the server's output line
_test-log-host: (_host-test 'log-server' 'protocol')

# Run the chainboot protocol's host tests and chainofcommand's unit tests
_test-chainboot-host: (_host-test 'vesper-chainboot-protocol' 'chainboot_protocol')
    cargo test -p chainofcommand

# === Lints ===

# Cross-clippy the workspace for a board with a feature set
_cross-clippy board='rpi3' features='':
    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ fixed_rustflags }} {{ if board == 'rpi3' { board_rpi3_flags } else { board_rpi4_flags } }}" \
    cargo clippy {{ target_json }} \
      {{ if features != '' { '--features=' + features } else { '' } }} \
      {{ rust_std }} \
      --workspace --exclude=chainofcommand --exclude=vesper-image-build --exclude=metta-meddle --exclude=meddler \
      -- --deny warnings --allow deprecated

# The images the workspace clippy's build scripts embed (the kernel, the components, log-test's programs)
_clippy-artifacts: (build 'rpi3' 'qemu') (_build-components 'rpi3' 'qemu') (_build-log-probe 'rpi3' 'qemu')

# Run embedded clippy checks (all feature combos) and the host-test linting
[group("lint")]
clippy: _clippy-artifacts (_cross-clippy 'rpi3' '') (_cross-clippy 'rpi4' '') (_cross-clippy 'rpi3' 'noserial') (_cross-clippy 'rpi3' 'qemu') (_cross-clippy 'rpi3' 'noserial,qemu') (_cross-clippy 'rpi3' 'jtag') (_cross-clippy 'rpi3' 'noserial,jtag') (build 'rpi3' 'qemu,debug_kernel') (_cross-clippy 'rpi3' 'debug_kernel') (_cross-clippy 'rpi3' 'qemu,debug_kernel') (_cross-clippy 'rpi3' 'qemu,fp_trap_test') (_cross-clippy 'rpi3' 'mmu') _clippy-host

# Run shortened clippy (default features on both boards) and the host-test linting
_clippy-pre-push: _clippy-artifacts (_cross-clippy 'rpi3' '') (_cross-clippy 'rpi4' '') _clippy-host

# Lint every host test and host tool
_clippy-host: _clippy-object-host _clippy-platform-host _clippy-meddle _clippy-usb-host _clippy-input-host _clippy-compositor-host _clippy-log-host _clippy-coc

# Lint one opt-in (`host-tests`) integration test with its native host harness
_host-clippy package test features='host-tests':
    RUSTFLAGS="{{ fixed_rustflags }}" \
    cargo clippy -p {{ package }} --features={{ features }} --test {{ test }} \
      -- --deny warnings --allow deprecated

# Lint the opt-in capability ABI tests with their native host harness
_clippy-object-host: (_host-clippy 'vesper-objects' 'object_type') (_host-clippy 'vesper-objects' 'object_type' 'host-tests,debug_kernel')

# Lint the boot-platform host tests
_clippy-platform-host: (_host-clippy 'vesper-devicetree' 'devicetree') (_host-clippy 'vesper-image' 'privileged') (_host-clippy 'irqchip-bcm2836' 'bcm2836') (_host-clippy 'irqchip-gicv2' 'gicv2') (_host-clippy 'vesper-io-channel' 'io_channel') (_host-clippy 'vesper-serial-channel' 'serial_channel') (_host-clippy 'vesper-block-channel' 'block_channel') (_host-clippy 'uart-pl011' 'pl011') (_host-clippy 'sdhost-bcm2835' 'sdhost') (_host-clippy 'gpio-bcm2835' 'gpio') (_host-clippy 'vesper-device-driver' 'areas') (_host-clippy 'vesper-device-driver' 'masters') (_host-clippy 'mailbox-impl' 'tags') (_host-clippy 'power-impl' 'pm') (_host-clippy 'cprman-bcm2835' 'cprman') (_host-clippy 'dma-bcm2835' 'dma') (_host-clippy 'audio-pwm-bcm2835' 'pwm') (_host-clippy 'audio-pwm-bcm2835' 'stream') (_host-clippy 'fb-impl' 'requests') (_host-clippy 'shell' 'commands') (_host-clippy 'vesper-graphics' 'graphics')

# Lint the Meddle interface compiler (library, meddler and tests) and the runtime host tests
_clippy-meddle: (_host-clippy 'metta-meddle-runtime' 'words')
    cargo clippy -p metta-meddle -p meddler --tests -- --deny warnings --allow deprecated

# Lint the USB protocol host tests with their native host harness
_clippy-usb-host: (_host-clippy 'vesper-usb-protocol' 'protocol') (_host-clippy 'usb-hcd-dwc2' 'dwc2') (_host-clippy 'usb-core' 'usb_core') (_host-clippy 'usb-hid' 'hid')

# Lint the input event library tests with their native host harness
_clippy-input-host: (_host-clippy 'vesper-input-event' 'input_event')

# Lint the compositor logic tests with their native host harness
_clippy-compositor-host: (_host-clippy 'metta-compositor-core' 'compositor_core')

# Lint the log server's host tests with their native host harness
_clippy-log-host: (_host-clippy 'log-server' 'protocol')

# Clippy for the host tools (chainofcommand and its protocol crate's host tests, vesper-image-build)
_clippy-coc: (_host-clippy 'vesper-chainboot-protocol' 'chainboot_protocol')
    cargo clippy -p vesper-image-build -- --deny warnings --allow deprecated # FIXME Shouldn't be here!
    cargo clippy -p chainofcommand --all-targets -- --deny warnings --allow deprecated

# Cross-clippy only the USB stack crates (controller, core, host library, HID) for a board with a feature set
_cross-clippy-usb board='rpi3' features='':
    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ fixed_rustflags }} {{ if board == 'rpi3' { board_rpi3_flags } else { board_rpi4_flags } }}" \
    cargo clippy {{ target_json }} \
      {{ if features != '' { '--features=' + features } else { '' } }} \
      {{ rust_std }} \
      -p usb-hcd-dwc2 -p usb-core -p vesper-usb-host -p vesper-usb-protocol -p usb-hid -p input-trace \
      -- --deny warnings --allow deprecated

# Lint just the USB stack: the target build with and without `qemu`, and the host tests (much shorter than `clippy`)
_clippy-usb: (_cross-clippy-usb 'rpi3' '') (_cross-clippy-usb 'rpi3' 'qemu') _clippy-usb-host

# Cross-clippy only the log crates (the log server component, its log_v1 interface crates, the client library, libserial-trace, which sends through it, and log-test's probe) for a board with a feature set
_cross-clippy-log board='rpi3' features='':
    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ fixed_rustflags }} {{ if board == 'rpi3' { board_rpi3_flags } else { board_rpi4_flags } }}" \
    cargo clippy {{ target_json }} \
      {{ if features != '' { '--features=' + features } else { '' } }} \
      {{ rust_std }} \
      -p log-server -p log-interface -p log-interface-client -p vesper-log-client -p vesper-serial-trace -p log-probe \
      -- --deny warnings --allow deprecated --allow clippy::map_unwrap_or --allow clippy::doc_markdown

# Lint just the log crates: the target build with and without `qemu`, and their host tests (much shorter than `clippy`)
_clippy-log: (_cross-clippy-log 'rpi3' '') (_cross-clippy-log 'rpi3' 'qemu') _clippy-log-host

# Cross-clippy only the audio crates (the clock manager, DMA and PWM audio components, their interface crates and the audio test client) for a board with a feature set
_cross-clippy-audio board='rpi3' features='':
    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ fixed_rustflags }} {{ if board == 'rpi3' { board_rpi3_flags } else { board_rpi4_flags } }}" \
    cargo clippy {{ target_json }} \
      {{ if features != '' { '--features=' + features } else { '' } }} \
      {{ rust_std }} \
      -p cprman-bcm2835 -p clock -p clock-client -p dma-bcm2835 -p dma -p dma-client -p audio-pwm-bcm2835 -p audio -p audio-client -p audio-test-client \
      -- --deny warnings --allow deprecated

# Lint just the audio crates: the target build with and without `qemu`, and their host tests (much shorter than `clippy`)
_clippy-audio: (_cross-clippy-audio 'rpi3' '') (_cross-clippy-audio 'rpi3' 'qemu') (_host-clippy 'cprman-bcm2835' 'cprman') (_host-clippy 'dma-bcm2835' 'dma') (_host-clippy 'audio-pwm-bcm2835' 'pwm') (_host-clippy 'audio-pwm-bcm2835' 'stream')

# Check formatting
[group("maintenance")]
fmt-check:
    cargo +nightly fmt -- --check

# Audit the integer-only FP/SIMD policy: no linked image that runs under it may contain an
# FP/SIMD instruction or register access (fp-trap-test and fp-probe execute one on purpose)
_audit-fp-simd: (build 'rpi3' 'qemu') (_build-test-kernel 'capability-test') (_build-test-kernel 'memory-test') (_build-test-kernel 'sync-test') (_build-test-kernel 'ppc-test') (_build-preempt-test 'rpi3') (_build-dispatch-test 'rpi3') (_build-irq-test 'rpi3') (_build-driver-test 'rpi3') (_build-mailbox-test 'rpi3') (_build-uart-test 'rpi3') (_build-fault-test 'rpi3') (_build-endpoint-test 'rpi3' 'qemu')
    #!/usr/bin/env bash
    set -euo pipefail
    # The production kernel and the qemu-only test kernels build in rpi3-qemu, the
    # debug_kernel test kernels in rpi3-qemu+debug_kernel; every copy of an image is audited.
    configs="{{ rpi3_qemu }} rpi3-qemu+debug_kernel{{ profile_suffix }}"
    # An instruction line whose mnemonic is an FP one (f…), or whose operands name an
    # FP/SIMD register (b/h/s/d/q/v0..31, with an optional arrangement) or FPCR/FPSR.
    fp_simd='^[[:space:]]+[0-9a-f]+:[[:space:]]+(f[a-z0-9]*|[a-z0-9.]+[[:space:]].*\b([bhsdqv][0-9]{1,2}(\.[0-9]*[bhsdq])?|fpcr|fpsr)\b)'
    failed=0
    # fp-probe executes an FP/SIMD instruction on purpose. The driver packages name their
    # binaries after the service (mailbox-impl builds `mailbox`).
    user_components="{{ replace(replace(replace(replace(user_components, 'fp-probe', ''), 'mailbox-impl', 'mailbox'), 'power-impl', 'power'), 'fb-impl', 'fb') }}"
    for image in nucleus kickstart {{ privileged_components }} capability-test memory-test sync-test ppc-test preempt-test dispatch-test irq-test driver-test mailbox-test uart-test endpoint-test fault-test ${user_components}; do
        audited=0
        for config in ${configs}; do
            elf="{{ target_root }}/${config}/{{ target }}/{{ image_profile }}/${image}"
            [ -f "${elf}" ] || continue
            audited=1
            # Symbol names in <…> are not operands.
            found=$(rust-objdump -d --no-show-raw-insn "${elf}" | sed 's/<[^>]*>//g' | grep -E "${fp_simd}" || true)
            if [ -n "${found}" ]; then
                echo "❌ ${image} (${config}) contains FP/SIMD instructions:"
                echo "${found}"
                failed=1
            else
                echo "{{ ok_label }} ${image} (${config}): integer-only"
            fi
        done
        if [ "${audited}" = 0 ]; then
            echo "❌ ${image}: not built in any of ${configs}"
            failed=1
        fi
    done
    exit "${failed}"

# Run every lint: formatting, clippy for every feature set and the host tests, the FP/SIMD audit
[group("lint")]
lint: fmt-check clippy _audit-fp-simd

# === CI ===

# Run pre-push local checks
[group("ci")]
pre-push: fmt-check _clippy-pre-push test

# Run CI tasks
[group("ci")]
ci: clean lint build test

# === Maintenance & Tools ===

# Generate and open documentation
[group("maintenance")]
doc:
    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" cargo doc --open --no-deps {{ target_json }} {{ rust_std }}

# Clean project
[group("maintenance")]
clean:
    cargo clean

# Update all dependencies
[group("maintenance")]
deps-up:
    cargo update

# Fetch the git submodules, prepare local dev tools and set-up git hooks
[group("maintenance")]
setup-local-dev:
    git submodule update --init --recursive
    which cargo-binstall || cargo install cargo-binstall
    commit-emoji --help || cargo binstall -y commit-emoji
    commit-emoji -i
    cargo binstall -y cargo-binutils
    # todo install rustfilt, what else?
    # install pre-push git hook with `just pre-push`

# === Modules dependency visualization ===

# Render modules dependency tree
[group("modules")]
modules:
    cargo modules tree

# Render modules dependency tree with versions
[group("modules")]
tree:
    cargo tree

_gen-deps-graph mod:
    cargo modules dependencies --max-depth 5 --no-sysroot --no-externs -p {{ mod }} > target/{{ mod }}.dot \
    && dot -Tpng target/{{ mod }}.dot -o target/{{ mod }}.png

# Render modules' usage graph
[macos]
_deps-graph mod: (_gen-deps-graph mod)
    open target/{{ mod }}.png

# Render modules' usage graph
[windows]
_deps-graph mod: (_gen-deps-graph mod)
    start target/{{ mod }}.png

# Render modules' usage graph
[linux]
_deps-graph mod: (_gen-deps-graph mod)
    xdg-open target/{{ mod }}.png

# Render modules symbol visibility
_exports mod:
    cargo modules structure -p {{ mod }}

# Find orphan files
[group("modules")]
orphans mod:
    cargo modules orphans -p {{ mod }}
