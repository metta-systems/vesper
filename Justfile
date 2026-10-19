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

nucleus_link    := 'libs/platform/src/raspberrypi/linker/nucleus.ld'
init_link       := 'libs/platform/src/raspberrypi/linker/kickstart.ld'
user_link       := 'userspace/runtime/user.ld'
test_link       := 'libs/platform/src/raspberrypi/linker/test.ld'
chainboot_link  := 'bin/chainboot/src/link.ld'
privileged_link := 'drivers/privileged.ld'

fixed_rustflags := '-D warnings -Z macro-backtrace'
# Privileged (EL1) components: position-independent, linked at 0 and relocated by kickstart
pie_rustflags   := '-C relocation-model=pie -C link-arg=--pie'
# Device tests run in QEMU (rpi3) with the test linker script
device_test_rustflags := fixed_rustflags + ' ' + board_rpi3_flags + ' -C link-arg=--script=' + test_link

# Sort components alphabetically!
# EL0 userspace components, linked with the userspace runtime's user.ld
user_components := 'dispatch-actor dispatch-child dispatch-dispatcher driver-probe endpoint-client endpoint-component endpoint-server fault-bare fault-faulter fb-impl fp-probe hello irq-receiver mailbox-impl mailbox-test-client power-impl preempt-reader preempt-spinner root-dispatcher shell uart-pl011 usb-hcd-dwc2 usb-test-client'
# Privileged components every kickstart image bundles (see kernel/kickstart/image.toml)
privileged_components := 'irqchip-bcm2836 irqchip-gicv2'
# EL0 driver and program components the kickstart image bundles (see kernel/kickstart/image.toml)
driver_components := 'fb-impl mailbox-impl power-impl root-dispatcher shell uart-pl011 usb-hcd-dwc2'

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

chainboot_serial := '/dev/tty.SLAB_USBtoUART'
chainboot_baud   := '115200'

# QEMU option fragments
# Console, monitor and semihosting wiring shared by every QEMU run
qemu_stdio_opts   := '-chardev stdio,mux=on,id=char0,logfile=qemu.log,signal=off -object monitor-hmp,chardev=char0,id=mon0 -semihosting-config enable=on,userspace=on,chardev=char0'
qemu_io_opts      := '-serial chardev:char0 ' + qemu_stdio_opts
qemu_base_opts    := '-M ' + qemu_machine + ' ' + qemu_io_opts
qemu_disasm       := '-d in_asm,unimp,int,mmu,cpu_reset,guest_errors,nochain,plugin'
qemu_gdb_opts     := '-gdb tcp::5555 -S'
qemu_test_opts    := '-nographic'
qemu_disasm_gdb   := qemu_disasm + ' ' + qemu_gdb_opts
# Boot a test image (rpi3); in-guest assertions and the QEMU exit status are the result
qemu_test         := qemu + ' ' + qemu_base_opts + ' ' + qemu_test_opts + ' -dtb "' + rpi3_dtb + '"'

gdb_connect     := justfile_directory() / 'target' / target / 'gdb-connect'

openocd_bin     := env('OPENOCD', '/usr/local/opt/openocd/4d6519593-rtt/bin/openocd')

ok_label        := '✅'
copy_label      := '🔄'

_default:
    @just --list

# === Low-level: cross-compile a single crate ===

# Cross-build crates (space-separated) for a board with a linker script, features and extra
# rustflags, into the target directory of the image configuration `board` + `image_features`
# (the features of the image they are bundled into, which may differ from their own)
[private]
_cross-build crates board='rpi4' linker_script='' features='' rustflags='' image_features=features:
    CARGO_TARGET_DIR="{{ target_root / (board + (if image_features == '' { '' } else { '-' + replace(image_features, ',', '+') }) + profile_suffix) }}" \
    RUSTFLAGS="{{ fixed_rustflags }} {{ if board == 'rpi3' { board_rpi3_flags } else { board_rpi4_flags } }}{{ if linker_script != '' { ' -C link-arg=--script=' + linker_script } else { '' } }}{{ if rustflags != '' { ' ' + rustflags } else { '' } }}" \
    cargo build {{ target_json }} \
      {{ if features != '' { '--features=' + features } else { '' } }} \
      {{ rust_std }} \
      --profile {{ cargo_profile }} -p {{ replace(crates, ' ', ' -p ') }}

# === Kernel (nucleus + kickstart -> kernel-<config>.bin) ===

# Build the privileged components every kickstart image bundles (see kernel/kickstart/image.toml)
[group("hw")]
build-privileged board='rpi4' image_features='': (_cross-build privileged_components board privileged_link '' pie_rustflags image_features)

# Build the EL0 driver components the kickstart image bundles (with semihosting output under 'qemu')
[group("hw")]
build-drivers board='rpi4' features='' image_features=features:
    {{ just_executable() }} _cross-build '{{ driver_components }}' '{{ board }}' '{{ user_link }}' '{{ if features =~ "qemu" { "qemu" } else { "" } }}' '' '{{ image_features }}'

# Build kernel (features: '' for hw, 'qemu' for emulation) into target/images/kernel-<config>.bin
[group("hw")]
build board='rpi4' features='' config=(board + (if features == '' { '' } else { '-' + replace(features, ',', '+') }) + profile_suffix): (_cross-build 'nucleus' board nucleus_link features) (build-privileged board features) (build-drivers board features) (_cross-build 'kickstart' board init_link features)
    @mkdir -p "{{ images_dir }}"
    {{ objcopy }} --strip-all -O binary "{{ target_root / config / target / image_profile / 'kickstart' }}" "{{ images_dir / 'kernel-' + config + '.bin' }}"
    @# TODO: print final binary size!
    @echo "{{ok_label}} kernel built for {{ board }}{{ if features != '' { ' [' + features + ']' } else { '' } }}: {{ images_dir / 'kernel-' + config + '.bin' }}"

alias b := build

# Build the EL0 userspace components (linked with the userspace runtime's user.ld) for the
# image configuration `board` + `image_features`
[group("emu")]
build-components board='rpi3' features='qemu' image_features=features: (_cross-build user_components board user_link features '' image_features)

# Build the endpoint-test e2e kernel (three-party rendezvous through an endpoint component)
[group("test")]
build-endpoint-test board='rpi3' features='qemu': (build-components board features) (_build-test-kernel 'endpoint-test' board features)

# Build the fp-trap-test negative e2e kernel (its nucleus carries the test-only `fp_trap_test` trap hook)
[group("test")]
build-fp-trap-test board='rpi3': (build-components board 'qemu' 'qemu,fp_trap_test') (_build-test-kernel 'fp-trap-test' board 'qemu,fp_trap_test')

# Build the preempt-test e2e kernel (timer tick and preemption of EL1t and EL0 Threads)
[group("test")]
build-preempt-test board='rpi3': (build-components board 'qemu' test_kernel_features) (_build-test-kernel 'preempt-test' board)

# Build the dispatch-test e2e kernel (a userspace root dispatcher bound to the Scheduler)
[group("test")]
build-dispatch-test board='rpi3': (build-components board 'qemu' test_kernel_features) (_build-test-kernel 'dispatch-test' board)

# Build the driver-test e2e kernel (Kickstart driver matching and the driver init page)
[group("test")]
build-driver-test board='rpi3': (build-components board 'qemu' test_kernel_features) (_build-test-kernel 'driver-test' board)

# Build the usb-test e2e kernel (the DWC2 host controller component, loaded by Kickstart)
[group("test")]
build-usb-test board='rpi3': (build-components board 'qemu' test_kernel_features) (_build-test-kernel 'usb-test' board)

# Boot usb-test on raspi3b with a hub on the root port: the DWC2 component resets the port and enumerates the hub
[group("test")]
test-usb board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (build-usb-test board)
    {{ qemu }} -M raspi3b {{ qemu_io_opts }} {{ qemu_test_opts }} -device usb-hub \
      -dtb "{{ rpi3_dtb }}" -kernel "{{ images_dir / 'usb-test-' + config + '.bin' }}"

# Build the mailbox-test e2e kernel (the mailbox component and a client wired through the firmware node)
[group("test")]
build-mailbox-test board='rpi3': (build-components board 'qemu' test_kernel_features) (_build-test-kernel 'mailbox-test' board)

# Build the irq-test e2e kernel (interrupt delivery to an EL0 receiver Thread)
[group("test")]
build-irq-test board='rpi3': (build-components board 'qemu' test_kernel_features) (_build-test-kernel 'irq-test' board)

# Build the uart-test e2e kernel (the PL011 driver component serving a client's IO channels in loopback)
[group("test")]
build-uart-test board='rpi3': (build-components board 'qemu' test_kernel_features) (_build-test-kernel 'uart-test' board)

# Build the fault-test e2e kernel (fault delivery to EL0 fault handlers)
[group("test")]
build-fault-test board='rpi3': (build-components board 'qemu') (_build-test-kernel 'fault-test' board 'qemu')

# === Chainboot ===

# Build chainboot bootloader (features: '' for hw, 'qemu' for emulation) into target/images/chainboot-<config>.bin
[group("hw")]
build-chainboot board='rpi4' features='' config=(board + (if features == '' { '' } else { '-' + replace(features, ',', '+') }) + profile_suffix): (_cross-build 'chainboot' board chainboot_link features)
    @mkdir -p "{{ images_dir }}"
    {{ objcopy }} --strip-all -O binary "{{ target_root / config / target / image_profile / 'chainboot' }}" "{{ images_dir / 'chainboot-' + config + '.bin' }}"
    @echo "{{ok_label}} chainboot built for {{ board }}{{ if features != '' { ' [' + features + ']' } else { '' } }}: {{ images_dir / 'chainboot-' + config + '.bin' }}"

# === Chainofcommand (host tool) ===

# Build chainofcommand serial loader
[group("hw")]
chainofcommand:
    @cargo build -p chainofcommand
    @echo "Run 'just boot' to boot via coc"

alias coc := chainofcommand

# === Meddler (host tool) ===

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

# === Zellij (QEMU in split terminal) ===

# The zellij runs name their serial ports themselves (the PL011, then the mini UART, each
# on its own pane), so only the monitor and semihosting stay on stdio
[private]
_write-zellij-config bin runner_opts dtb machine=qemu_machine:
    #!/usr/bin/env bash
    cat > emulation/zellij-config.sh <<EOF
    QEMU="{{ qemu }}"
    QEMU_OPTS="-M {{ machine }} {{ qemu_stdio_opts }}"
    QEMU_RUNNER_OPTS="{{ runner_opts }}"
    CARGO_MAKE_WORKSPACE_WORKING_DIRECTORY="{{ justfile_directory() }}"
    TARGET_DTB="{{ dtb }}"
    KERNEL_BIN="{{ bin }}"
    EOF

# Build and run kernel in QEMU with serial port emulation: the PL011 and the mini UART each get a pane
[group("emu")]
zellij: (build 'rpi3' 'qemu') (_write-zellij-config kernel_rpi3_qemu_bin '' rpi3_dtb)
    zellij --layout emulation/layout.zellij

# Like zellij, but QEMU waits for GDB on port 5555 and traces instructions
[group("emu")]
zellij-gdb: (build 'rpi3' 'qemu') (_write-zellij-config kernel_rpi3_qemu_bin qemu_disasm_gdb rpi3_dtb)
    zellij --layout emulation/layout.zellij

alias z-qemu := zellij

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

# === GDB ===

[private]
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

# === SD Card ===

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
cb-eject: build-chainboot
    cp "{{ chainboot_rpi4_bin }}" "{{ volume }}/chain_boot_rpi4.img"
    @echo "{{copy_label}} copied chainboot to {{ volume }}/chain_boot_rpi4.img"
    diskutil ejectAll "{{ volume }}"

# Build and boot via chainofcommand
[group("hw")]
boot: build chainofcommand
    target/debug/chainofcommand {{ chainboot_serial }} {{ chainboot_baud }} --kernel "{{ kernel_rpi4_bin }}"

# Build and boot in qemu via chainofcommand
[group("emu")]
boot-qemu: (build 'rpi3' 'qemu') chainofcommand
    target/debug/chainofcommand {{ chainboot_serial }} {{ chainboot_baud }} --kernel "{{ kernel_rpi3_qemu_bin }}"

# === Openocd ===

# Start openocd connected via JTAG
[group("hw")]
openocd board='rpi4':
    {{ openocd_bin }} -f interface/jlink.cfg -f ../ocd/{{ board }}_target.cfg

alias ocd := openocd

# === Testing ===

# Run device and chainboot tests in QEMU (rpi3), plus capability and tool tests natively
[group("test")]
test: test-device test-chainboot test-host test-debug-console test-key-table test-untyped test-capability test-memory test-sync test-ppc test-preempt test-preempt-rpi4 test-dispatch test-dispatch-rpi4 test-irq test-irq-rpi4 test-driver test-driver-rpi4 test-mailbox test-mailbox-rpi4 test-uart test-uart-rpi4 test-endpoint test-fp-trap test-fault test-usb

alias t := test

# Run device crate tests in QEMU (rpi3) --verbose
[group("test")]
test-device: (_cross-build 'nucleus' 'rpi3' nucleus_link 'qemu') (build-privileged 'rpi3' 'qemu') (build-drivers 'rpi3' 'qemu')
    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ device_test_rustflags }}" \
    cargo test --tests {{ target_json }} --features=qemu {{ rust_std }} \
      --workspace --exclude=chainofcommand --exclude=vesper-image-build --exclude=vesper-meddle --exclude=meddler --exclude=chainboot

    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ device_test_rustflags }}" \
    cargo test --doc {{ target_json }} --features=qemu {{ rust_std }} \
    --workspace --exclude=chainofcommand --exclude=vesper-image-build --exclude=vesper-meddle --exclude=meddler --exclude=chainboot

# Run one nucleus integration test binary in QEMU (rpi3)
[private]
_test-nucleus test features='qemu':
    RUSTFLAGS="{{ device_test_rustflags }}" \
    cargo test -p nucleus --test {{ test }} {{ target_json }} \
      --features={{ features }} {{ rust_std }}

# Test the debug-only nucleus console handler and the scheduler/PPC suites in QEMU (rpi3)
[group("test")]
test-debug-console: (_test-nucleus 'debug_console' 'qemu,debug_kernel')

# Test the nucleus KeyTable management handler in QEMU (rpi3)
[group("test")]
test-key-table: (_test-nucleus 'key_table' 'qemu,debug_kernel')

# Test the nucleus Untyped Retype handler in QEMU (rpi3)
[group("test")]
test-untyped: (_test-nucleus 'untyped')

# Build one Kickstart-based e2e test kernel: the nucleus, the privileged
# components, then `crate` linked as the init image, then its raw binary.
# Kernels bundling userspace components build them first (build-endpoint-test …).
[private]
_build-test-kernel crate board='rpi3' features=test_kernel_features config=(board + (if features == '' { '' } else { '-' + replace(features, ',', '+') }) + profile_suffix): (_cross-build 'nucleus' board nucleus_link features) (build-privileged board features) (_cross-build crate board init_link features)
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
# invocation always runs and refreshes the image.
[private]
_run-test-kernel crate build=('_build-test-kernel ' + crate) board='rpi3' features=test_kernel_features config=(board + (if features == '' { '' } else { '-' + replace(features, ',', '+') }) + profile_suffix):
    {{ just_executable() }} {{ build }}
    {{ qemu }} -M {{ if board == 'rpi3' { 'raspi3b' } else { 'raspi4b' } }} {{ qemu_io_opts }} {{ qemu_test_opts }} \
      -dtb "{{ if board == 'rpi3' { rpi3_dtb } else { rpi4_dtb } }}" \
      -kernel "{{ images_dir / crate + '-' + config + '.bin' }}"

# Boot capability-test: boot-table invariants, debug console key, KeyTable/Frame Retype, Untyped split
[group("test")]
test-capability: (_run-test-kernel 'capability-test')

# Boot ppc-test: Invocation construction and same-Thread PPC Call/Return into the Bounce AddressSpace
[group("test")]
test-ppc: (_run-test-kernel 'ppc-test')

# Boot preempt-test: interrupt-controller component, kernel timer tick and round-robin preemption of Threads that never yield
[group("test")]
test-preempt: (_run-test-kernel 'preempt-test' 'build-preempt-test')

# Boot preempt-test on the Raspberry Pi 4 machine: the same image, ticking through the GICv2 component
[group("test")]
test-preempt-rpi4: (_run-test-kernel 'preempt-test' 'build-preempt-test rpi4' 'rpi4')

# Boot dispatch-test: a userspace root dispatcher runs worker Threads round-robin through Scheduler.Dispatch
[group("test")]
test-dispatch: (_run-test-kernel 'dispatch-test' 'build-dispatch-test')

# Boot dispatch-test on the Raspberry Pi 4 machine
[group("test")]
test-dispatch-rpi4: (_run-test-kernel 'dispatch-test' 'build-dispatch-test rpi4' 'rpi4')

# Boot driver-test: Kickstart loads a probe driver per matching device tree node and hands it registers, interrupts, memory and a Time
[group("test")]
test-driver: (_run-test-kernel 'driver-test' 'build-driver-test')

# Boot driver-test on the Raspberry Pi 4 machine: the same checks through the GICv2 component
[group("test")]
test-driver-rpi4: (_run-test-kernel 'driver-test' 'build-driver-test rpi4' 'rpi4')

# Boot mailbox-test: Kickstart loads the mailbox component (through the kernel-owned page on the Raspberry Pi 3) and a client on the firmware power node that calls it
[group("test")]
test-mailbox: (_run-test-kernel 'mailbox-test' 'build-mailbox-test')

# Boot mailbox-test on the Raspberry Pi 4 machine: the mailbox page is an ordinary device page there
[group("test")]
test-mailbox-rpi4: (_run-test-kernel 'mailbox-test' 'build-mailbox-test rpi4' 'rpi4')

# Boot the production image (rpi3, QEMU) and type commands at the shell on the serial console (the kernel's semihosting output goes to its own file), from a slow feeder so the UART's FIFO never overflows: the commands must answer, and `end` must shut QEMU down with a success status
[group("test")]
test-shell: (build 'rpi3' 'qemu')
    @rm -f qemu.log target/shell-test.log target/shell-semihosting.log
    (sleep 10; printf 'help\n'; sleep 2; printf 'led on\n'; sleep 2; printf 'led off\n'; sleep 2; printf 'feats\n'; sleep 2; printf 'draw\n'; sleep 2; printf 'end\n'; sleep 3) | {{ qemu }} -M raspi3b -display none -monitor none -chardev stdio,id=uart,signal=off -serial chardev:uart -chardev file,id=semihosting,path=target/shell-semihosting.log -semihosting-config enable=on,userspace=on,chardev=semihosting -dtb "{{ rpi3_dtb }}" -kernel "{{ kernel_rpi3_qemu_bin }}" > target/shell-test.log
    @grep -q "Supported console commands" target/shell-test.log || { echo "shell-test: help printed nothing"; exit 1; }
    @grep -q "Unknown command feats, try 'help'" target/shell-test.log || { echo "shell-test: an unknown command was not reported"; exit 1; }
    @if grep -q "led on failed\|led off failed" target/shell-test.log; then echo "shell-test: the LED command failed"; exit 1; fi
    @grep -q "font: 95 glyphs mapped" target/shell-test.log || { echo "shell-test: the font resource was not mapped"; exit 1; }
    @if grep -q "draw failed" target/shell-test.log; then echo "shell-test: draw failed"; exit 1; fi
    @grep -q "lamp: frame 1, [1-9][0-9]* sampled cells lit" target/shell-semihosting.log || { echo "shell-test: the lamp drew nothing"; exit 1; }
    @grep -q "lamp: frame 20," target/shell-semihosting.log || { echo "shell-test: the lamp stopped animating"; exit 1; }
    @grep -q "Bye" target/shell-test.log || { echo "shell-test: end did not say goodbye"; exit 1; }
    @echo "{{ ok_label }} shell answered help, the LED commands, an unknown command and end"

# Boot uart-test: the PL011 driver component's two Threads serve a client's transmit and receive IO channels in loopback by interrupts
[group("test")]
test-uart: (_run-test-kernel 'uart-test' 'build-uart-test')

# Boot uart-test on the Raspberry Pi 4 machine: the same exchange through the GICv2 component
[group("test")]
test-uart-rpi4: (_run-test-kernel 'uart-test' 'build-uart-test rpi4' 'rpi4')

# Boot irq-test: IRQControl.Issue, IRQHandler binding and Ack, repeated virtual-timer interrupts delivered to an EL0 receiver Thread
[group("test")]
test-irq: (_run-test-kernel 'irq-test' 'build-irq-test')

# Boot irq-test on the Raspberry Pi 4 machine: the same checks through the GICv2 component
[group("test")]
test-irq-rpi4: (_run-test-kernel 'irq-test' 'build-irq-test rpi4' 'rpi4')

# Boot sync-test: Notification, EventCount, blocking waits through the Bounce Thread, Thread.Retire
[group("test")]
test-sync: (_run-test-kernel 'sync-test')

# Boot memory-test: PageTable/Frame mapping, alias policy, ASIDs, activation and TLB invalidation, AddressSpace.Retire
[group("test")]
test-memory: (_run-test-kernel 'memory-test')

# Boot endpoint-test: client, endpoint and server AddressSpaces rendezvous through PPC
[group("test")]
test-endpoint: (_run-test-kernel 'endpoint-test' 'build-endpoint-test' 'rpi3' 'qemu')

# Boot fp-trap-test: an FP/SIMD instruction must trap at EL1t and at EL0
[group("test")]
test-fp-trap: (_run-test-kernel 'fp-trap-test' 'build-fp-trap-test' 'rpi3' 'qemu,fp_trap_test')

# Boot fault-test: faults are delivered to EL0 fault handlers (skip, retry, terminate, Return faults) and every unhandled case parks the Thread
[group("test")]
test-fault: (_run-test-kernel 'fault-test' 'build-fault-test' 'rpi3' 'qemu')

# Run chainboot tests in QEMU (rpi3) with its own linker script
[group("test")]
test-chainboot:
    RUSTFLAGS="{{ fixed_rustflags }} {{ board_rpi3_flags }} -C link-arg=--script={{ chainboot_link }}" \
    cargo test {{ target_json }} --features=qemu {{ rust_std }} \
      -p chainboot

# Run capability, boot-platform and host tool tests natively
[group("test")]
test-host: test-object-host test-platform-host test-meddle test-usb-host test-input-host
    cargo test -p chainofcommand

# Run one opt-in (`host-tests`) integration test natively (currently AArch64 hosts)
[private]
_host-test package test features='host-tests':
    RUSTFLAGS="{{ fixed_rustflags }}" \
    cargo test -p {{ package }} --features={{ features }} --test {{ test }}

# Run the opt-in capability ABI tests on the native host
[group("test")]
test-object-host: (_host-test 'vesper-objects' 'object_type') (_host-test 'vesper-objects' 'object_type' 'host-tests,debug_kernel')

# Run the boot-platform tests natively: device tree helpers against the board
# DTBs in targets/, privileged-image loading, interrupt-controller register logic
[group("test")]
test-platform-host: (_host-test 'vesper-devicetree' 'devicetree') (_host-test 'vesper-image' 'privileged') (_host-test 'irqchip-bcm2836' 'bcm2836') (_host-test 'irqchip-gicv2' 'gicv2') (_host-test 'vesper-io-channel' 'io_channel') (_host-test 'vesper-serial-channel' 'serial_channel') (_host-test 'uart-pl011' 'pl011') (_host-test 'vesper-device-driver' 'areas') (_host-test 'vesper-device-driver' 'masters') (_host-test 'mailbox-impl' 'tags') (_host-test 'power-impl' 'pm') (_host-test 'fb-impl' 'requests') (_host-test 'shell' 'commands') (_host-test 'vesper-graphics' 'graphics')

# Run the Meddle interface compiler tests and the generated code's runtime tests natively
[group("test")]
test-meddle: (_host-test 'vesper-meddle-runtime' 'words')
    cargo test -p vesper-meddle

# Run the USB protocol logic tests natively: setup packets, descriptors, enumeration, HID boot reports
[group("test")]
test-usb-host: (_host-test 'vesper-usb-protocol' 'protocol') (_host-test 'usb-hcd-dwc2' 'dwc2')

# Run the input event library tests natively: record layout, descriptor, state tracker
[group("test")]
test-input-host: (_host-test 'vesper-input-event' 'input_event')

# Test runner invoked by .cargo/config.toml runner
[private]
_test-runner binary_path:
    #!/usr/bin/env bash
    set -euo pipefail
    name=$(basename "{{ binary_path }}")
    mkdir -p "{{ images_dir }}"
    bin="{{ images_dir }}/test-${name}.bin"
    {{ objcopy }} --strip-all -O binary "{{ binary_path }}" "${bin}"
    echo "🚨 Running test: ${name}"
    {{ qemu_test }} -kernel "${bin}"

# === Clippy ===

# Cross-clippy the workspace for a board with a feature set
[private]
_cross-clippy board='rpi3' features='':
    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ fixed_rustflags }} {{ if board == 'rpi3' { board_rpi3_flags } else { board_rpi4_flags } }}" \
    cargo clippy {{ target_json }} \
      {{ if features != '' { '--features=' + features } else { '' } }} \
      {{ rust_std }} \
      --workspace --exclude=chainofcommand --exclude=vesper-image-build --exclude=vesper-meddle --exclude=meddler \
      -- --deny warnings --allow deprecated

# Run embedded clippy checks (all feature combos) and capability host-test linting
[group("maintenance")]
clippy: (build 'rpi3' 'qemu') (build-components 'rpi3' 'qemu') (_cross-clippy 'rpi3' '') (_cross-clippy 'rpi4' '') (_cross-clippy 'rpi3' 'noserial') (_cross-clippy 'rpi3' 'qemu') (_cross-clippy 'rpi3' 'noserial,qemu') (_cross-clippy 'rpi3' 'jtag') (_cross-clippy 'rpi3' 'noserial,jtag') (build 'rpi3' 'qemu,debug_kernel') (_cross-clippy 'rpi3' 'debug_kernel') (_cross-clippy 'rpi3' 'qemu,debug_kernel') (_cross-clippy 'rpi3' 'qemu,fp_trap_test') clippy-object-host clippy-platform-host clippy-meddle clippy-usb-host clippy-input-host

# Run shortened clippy (default features on both boards) and capability host-test linting
[group("maintenance")]
clippy-pre-push: (build 'rpi3' 'qemu') (_cross-clippy 'rpi3' '') (_cross-clippy 'rpi4' '') clippy-object-host clippy-platform-host clippy-meddle clippy-usb-host clippy-input-host

# Lint one opt-in (`host-tests`) integration test with its native host harness
[private]
_host-clippy package test features='host-tests':
    RUSTFLAGS="{{ fixed_rustflags }}" \
    cargo clippy -p {{ package }} --features={{ features }} --test {{ test }} \
      -- --deny warnings --allow deprecated

# Lint the opt-in capability ABI tests with their native host harness
[group("maintenance")]
clippy-object-host: (_host-clippy 'vesper-objects' 'object_type') (_host-clippy 'vesper-objects' 'object_type' 'host-tests,debug_kernel')

# Lint the boot-platform host tests
[group("maintenance")]
clippy-platform-host: (_host-clippy 'vesper-devicetree' 'devicetree') (_host-clippy 'vesper-image' 'privileged') (_host-clippy 'irqchip-bcm2836' 'bcm2836') (_host-clippy 'irqchip-gicv2' 'gicv2') (_host-clippy 'vesper-io-channel' 'io_channel') (_host-clippy 'vesper-serial-channel' 'serial_channel') (_host-clippy 'uart-pl011' 'pl011') (_host-clippy 'vesper-device-driver' 'areas') (_host-clippy 'vesper-device-driver' 'masters') (_host-clippy 'mailbox-impl' 'tags') (_host-clippy 'power-impl' 'pm') (_host-clippy 'fb-impl' 'requests') (_host-clippy 'shell' 'commands') (_host-clippy 'vesper-graphics' 'graphics')

# Lint the Meddle interface compiler (library, meddler and tests) and the runtime host tests
[group("maintenance")]
clippy-meddle: (_host-clippy 'vesper-meddle-runtime' 'words')
    cargo clippy -p vesper-meddle -p meddler --tests -- --deny warnings --allow deprecated

# Lint the USB protocol host tests with their native host harness
[group("maintenance")]
clippy-usb-host: (_host-clippy 'vesper-usb-protocol' 'protocol') (_host-clippy 'usb-hcd-dwc2' 'dwc2')

# Lint the input event library tests with their native host harness
[group("maintenance")]
clippy-input-host: (_host-clippy 'vesper-input-event' 'input_event')

# Clippy for the host tools (chainofcommand, vesper-image-build)
[private]
_clippy-coc:
    cargo clippy -p vesper-image-build -- --deny warnings --allow deprecated # FIXME Shouldn't be here!
    cargo clippy -p chainofcommand -- --deny warnings --allow deprecated

# Build and disassemble kernel
[group("debug")]
hopper: build
    hopper --loader ELF --executable "{{ kernel_rpi4_elf }}"

alias disasm := hopper

[group("debug")]
cb-hopper: (build-chainboot 'rpi3' 'qemu')
    #hopper --loader RAW --plugin arm --cpu aarch64 --variant generic --base-address 0x80000 --executable "{{ chainboot_rpi3_qemu_bin }}"

alias cb-disasm := cb-hopper

# === Maintenance & Tools ===

# Build and print all symbols
[group("maintenance")]
nm: build
    {{ nm }} "{{ kernel_rpi4_elf }}" | sort -k 1 | rustfilt

# Run `cargo expand` on kernel
[group("maintenance")]
expand:
    cargo expand {{ target_json }} --release -- kernel

# Generate and open documentation
[group("maintenance")]
doc:
    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" cargo doc --open --no-deps {{ target_json }} {{ rust_std }}

# Clean project
[group("maintenance")]
clean:
    cargo clean

# Check formatting
[group("maintenance")]
fmt-check:
    cargo +nightly fmt -- --check

# Audit the integer-only FP/SIMD policy: no linked image that runs under it may contain an
# FP/SIMD instruction or register access (fp-trap-test and fp-probe execute one on purpose)
[group("test")]
audit-fp-simd: (build 'rpi3' 'qemu') (_build-test-kernel 'capability-test') (_build-test-kernel 'memory-test') (_build-test-kernel 'sync-test') (_build-test-kernel 'ppc-test') (build-preempt-test 'rpi3') (build-dispatch-test 'rpi3') (build-irq-test 'rpi3') (build-driver-test 'rpi3') (build-mailbox-test 'rpi3') (build-uart-test 'rpi3') (build-fault-test 'rpi3') (build-endpoint-test 'rpi3' 'qemu')
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

# Run lint tasks
[group("maintenance")]
lint: fmt-check clippy _clippy-coc audit-fp-simd

# Run pre-push local checks
[group("ci")]
pre-push: fmt-check clippy-pre-push test

# Run CI tasks
[group("ci")]
ci: clean lint build test

# Update all dependencies
[group("maintenance")]
deps-up:
    cargo update

# === Modules dependency visualization ===

# Render modules dependency tree
[group("modules")]
modules:
    cargo modules tree

# Render modules dependency tree with versions
[group("modules")]
tree:
    cargo tree

[private]
_gen-deps-graph mod:
    cargo modules dependencies --max-depth 5 --no-sysroot --no-externs -p {{ mod }} > target/{{ mod }}.dot \
    && dot -Tpng target/{{ mod }}.dot -o target/{{ mod }}.png

# Render modules' usage graph
[group("modules")]
[macos]
deps-graph mod: (_gen-deps-graph mod)
    open target/{{ mod }}.png

# Render modules' usage graph
[group("modules")]
[windows]
deps-graph mod: (_gen-deps-graph mod)
    start target/{{ mod }}.png

# Render modules' usage graph
[group("modules")]
[linux]
deps-graph mod: (_gen-deps-graph mod)
    xdg-open target/{{ mod }}.png

# Render modules symbol visibility
[group("modules")]
exports mod:
    cargo modules structure -p {{ mod }}

# Find orphan files
[group("modules")]
orphans mod:
    cargo modules orphans -p {{ mod }}

# Prepare local dev tools and set-up git hooks
[group("maintenance")]
setup-local-dev:
    which cargo-binstall || cargo install cargo-binstall
    commit-emoji --help || cargo binstall -y commit-emoji
    commit-emoji -i
    cargo binstall -y cargo-binutils
    # todo install rustfilt, what else?
    # install pre-push git hook with `just pre-push`
