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

# EL0 userspace components, linked with the userspace runtime's user.ld
user_components := 'hello endpoint-client endpoint-component endpoint-server fp-probe fault-faulter fault-bare preempt-spinner'
# Privileged components every kickstart image bundles (see kernel/kickstart/image.toml)
privileged_components := 'irqchip-bcm2836'

qemu            := env('QEMU', 'qemu-system-aarch64')
qemu_machine    := env('QEMU_MACHINE', 'raspi3b')
gdb             := env('GDB', 'aarch64-elf-gdb') # An aarch64-enabled GDB (brew install aarch64-elf-gdb)
objcopy         := 'rust-objcopy'
nm              := 'rust-nm'
volume          := env('VOLUME', '/Volumes/BOOT')

release_dir     := justfile_directory() / 'target' / target / 'release'
kernel_elf      := release_dir / 'kickstart'
kernel_bin      := justfile_directory() / 'target/kernel.bin'
chainboot_elf   := release_dir / 'chainboot'
chainboot_bin   := justfile_directory() / 'target/chainboot.bin'

chainboot_serial := '/dev/tty.SLAB_USBtoUART'
chainboot_baud   := '115200'

# QEMU option fragments
qemu_base_opts    := '-M ' + qemu_machine + ' -chardev stdio,mux=on,id=char0,logfile=qemu.log,signal=off -object monitor-hmp,chardev=char0,id=mon0 -serial chardev:char0 -semihosting-config enable=on,userspace=on,chardev=char0'
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

# Cross-build crates (space-separated) for a board with a linker script, features and extra rustflags
[private]
_cross-build crates board='rpi4' linker_script='' features='' rustflags='':
    RUSTFLAGS="{{ fixed_rustflags }} {{ if board == 'rpi3' { board_rpi3_flags } else { board_rpi4_flags } }}{{ if linker_script != '' { ' -C link-arg=--script=' + linker_script } else { '' } }}{{ if rustflags != '' { ' ' + rustflags } else { '' } }}" \
    cargo build {{ target_json }} \
      {{ if features != '' { '--features=' + features } else { '' } }} \
      {{ rust_std }} \
      --release -p {{ replace(crates, ' ', ' -p ') }}

# === Kernel (nucleus + kickstart -> kernel.bin) ===

# Build the privileged components every kickstart image bundles (see kernel/kickstart/image.toml)
[group("hw")]
build-privileged board='rpi4': (_cross-build privileged_components board privileged_link '' pie_rustflags)

# Build kernel (features: '' for hw, 'qemu' for emulation)
[group("hw")]
build board='rpi4' features='': (_cross-build 'nucleus' board nucleus_link features) (build-privileged board) (_cross-build 'kickstart' board init_link features)
    {{ objcopy }} --strip-all -O binary "{{ kernel_elf }}" "{{ kernel_bin }}"
    @# TODO: print final binary size!
    @echo "{{ok_label}} kernel built for {{ board }}{{ if features != '' { ' [' + features + ']' } else { '' } }}"

alias b := build

# Build the EL0 userspace components (linked with the userspace runtime's user.ld)
[group("emu")]
build-components board='rpi3' features='qemu': (_cross-build user_components board user_link features)

# Build the endpoint-test e2e kernel (three-party rendezvous through an endpoint component)
[group("test")]
build-endpoint-test board='rpi3' features='qemu': (build-components board features) (_build-test-kernel 'endpoint-test' board features)

# Build the fp-trap-test negative e2e kernel (its nucleus carries the test-only `fp_trap_test` trap hook)
[group("test")]
build-fp-trap-test board='rpi3': (build-components board 'qemu') (_build-test-kernel 'fp-trap-test' board 'qemu,fp_trap_test')

# Build the preempt-test e2e kernel (timer tick and preemption of EL1t and EL0 Threads)
[group("test")]
build-preempt-test board='rpi3': (build-components board 'qemu') (_build-test-kernel 'preempt-test' board)

# Build the fault-test e2e kernel (fault delivery to EL0 fault handlers)
[group("test")]
build-fault-test board='rpi3': (build-components board 'qemu') (_build-test-kernel 'fault-test' board 'qemu')

# === Chainboot ===

# Build chainboot bootloader (features: '' for hw, 'qemu' for emulation)
[group("hw")]
build-chainboot board='rpi4' features='': (_cross-build 'chainboot' board chainboot_link features)
    {{ objcopy }} --strip-all -O binary "{{ chainboot_elf }}" "{{ chainboot_bin }}"
    @echo "{{ok_label}} chainboot built for {{ board }}{{ if features != '' { ' [' + features + ']' } else { '' } }}"

# === Chainofcommand (host tool) ===

# Build chainofcommand serial loader
[group("hw")]
chainofcommand:
    @cargo build -p chainofcommand
    @echo "Run 'just boot' to boot via coc"

alias coc := chainofcommand

# === QEMU runners ===

# Build and run kernel in QEMU
[group("emu")]
qemu: (build 'rpi3' 'qemu')
    @echo "🚜 Run QEMU {{ qemu_base_opts }} with {{ kernel_bin }}"
    @echo "🚜 .. on {{ rpi3_dtb }}"
    @rm -f qemu.log
    {{ qemu }} {{ qemu_base_opts }} -dtb "{{ rpi3_dtb }}" -kernel "{{ kernel_bin }}"

# Build and run kernel in QEMU with GDB port
[group("emu")]
qemu-gdb: (build 'rpi3' 'qemu')
    @echo "🚜 Run QEMU {{ qemu_base_opts }} {{ qemu_disasm_gdb }} with {{ kernel_bin }}"
    @echo "🚜 .. on {{ rpi3_dtb }}"
    @rm -f qemu.log
    {{ qemu }} {{ qemu_base_opts }} {{ qemu_disasm_gdb }} -dtb "{{ rpi3_dtb }}" -kernel "{{ kernel_bin }}"

# Build and run chainboot in QEMU
[group("emu")]
cb-qemu: (build-chainboot 'rpi3' 'qemu')
    @echo "🚜 Run QEMU {{ qemu_base_opts }} {{ qemu_disasm }} with {{ chainboot_bin }}"
    @echo "🚜 .. on {{ rpi3_dtb }}"
    @rm -f qemu.log
    {{ qemu }} -serial tcp:127.0.0.1:4321,server,nowait {{ qemu_base_opts }} -dtb "{{ rpi3_dtb }}" -kernel "{{ chainboot_bin }}"

# Build and run chainboot in QEMU with GDB port
[group("emu")]
cb-qemu-gdb: (build-chainboot 'rpi3' 'qemu')
    @echo "🚜 Run QEMU {{ qemu_base_opts }} {{ qemu_disasm_gdb }} with {{ chainboot_bin }}"
    @echo "🚜 .. on {{ rpi3_dtb }}"
    @rm -f qemu.log
    {{ qemu }} {{ qemu_base_opts }} {{ qemu_disasm_gdb }} -serial pty -dtb "{{ rpi3_dtb }}" -kernel "{{ chainboot_bin }}"

# === Zellij (QEMU in split terminal) ===

[private]
_write-zellij-config bin runner_opts dtb:
    #!/usr/bin/env bash
    cat > emulation/zellij-config.sh <<EOF
    QEMU="{{ qemu }}"
    QEMU_OPTS="{{ qemu_base_opts }}"
    QEMU_RUNNER_OPTS="{{ runner_opts }}"
    CARGO_MAKE_WORKSPACE_WORKING_DIRECTORY="{{ justfile_directory() }}"
    TARGET_DTB="{{ dtb }}"
    KERNEL_BIN="{{ bin }}"
    EOF

# Build and run kernel in QEMU with serial port emulation
[group("emu")]
zellij: (build 'rpi3' 'qemu') (_write-zellij-config kernel_bin qemu_disasm_gdb rpi3_dtb)
    zellij --layout emulation/layout.zellij

alias z-qemu := zellij

# Build and run chainboot in QEMU with serial port emulation
[group("emu")]
cb-zellij: (build-chainboot 'rpi3' 'qemu') (_write-zellij-config chainboot_bin qemu_disasm rpi3_dtb)
    zellij --layout emulation/layout.zellij

# Run chainboot with GDB in zellij window
[group("emu")]
cb-zellij-gdb: (build-chainboot 'rpi3' 'qemu') (_write-zellij-config chainboot_bin qemu_disasm_gdb rpi3_dtb)
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
    @pipx run gdbgui -g "{{ gdb }} -x '{{ gdb_connect }}' '{{ kernel_elf }}'"

# Build and run chainboot in GDB
[group("debug")]
cb-gdb: build-chainboot _write-gdb-config
    {{ gdb }} -x "{{ gdb_connect }}" "{{ chainboot_elf }}"

# === SD Card ===

# Build and write kernel to SD Card
[group("hw")]
device: build
    cp "{{ kernel_bin }}" "{{ volume }}/kernel8.img"
    @echo "{{copy_label}} copied kernel to {{ volume }}/kernel8.img"

# Build and write kernel to SD Card, then eject
[group("hw")]
device-eject: device
    diskutil ejectAll "{{ volume }}"

# Build and write chainboot to SD Card, then eject
[group("hw")]
cb-eject: build-chainboot
    cp "{{ chainboot_bin }}" "{{ volume }}/chain_boot_rpi4.img"
    @echo "{{copy_label}} copied chainboot to {{ volume }}/chain_boot_rpi4.img"
    diskutil ejectAll "{{ volume }}"

# Build and boot via chainofcommand
[group("hw")]
boot: build chainofcommand
    target/debug/chainofcommand {{ chainboot_serial }} {{ chainboot_baud }} --kernel target/kernel.bin

# Build and boot in qemu via chainofcommand
[group("emu")]
boot-qemu: (build 'rpi3' 'qemu') chainofcommand
    target/debug/chainofcommand {{ chainboot_serial }} {{ chainboot_baud }} --kernel target/kernel.bin

# === Openocd ===

# Start openocd connected via JTAG
[group("hw")]
openocd board='rpi4':
    {{ openocd_bin }} -f interface/jlink.cfg -f ../ocd/{{ board }}_target.cfg

alias ocd := openocd

# === Testing ===

# Run device and chainboot tests in QEMU (rpi3), plus capability and tool tests natively
[group("test")]
test: test-device test-chainboot test-host test-debug-console test-key-table test-untyped test-capability test-memory test-sync test-ppc test-preempt test-endpoint test-fp-trap test-fault

alias t := test

# Run device crate tests in QEMU (rpi3) --verbose
[group("test")]
test-device:
    RUSTFLAGS="{{ device_test_rustflags }}" \
    cargo test --tests {{ target_json }} --features=qemu {{ rust_std }} \
      --workspace --exclude=chainofcommand --exclude=vesper-image-build --exclude=chainboot

    RUSTFLAGS="{{ device_test_rustflags }}" \
    cargo test --doc {{ target_json }} --features=qemu {{ rust_std }} \
    --workspace --exclude=chainofcommand --exclude=vesper-image-build --exclude=chainboot

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
_build-test-kernel crate board='rpi3' features='qemu,debug_kernel': (_cross-build 'nucleus' board nucleus_link features) (build-privileged board) (_cross-build crate board init_link features)
    {{ objcopy }} --strip-all -O binary "{{ release_dir / crate }}" "{{ justfile_directory() / 'target' / crate + '.bin' }}"
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
_run-test-kernel crate build=('_build-test-kernel ' + crate):
    {{ just_executable() }} {{ build }}
    {{ qemu_test }} -kernel "{{ justfile_directory() / 'target' / crate + '.bin' }}"

# Boot capability-test: boot-table invariants, debug console key, KeyTable/Frame Retype, Untyped split
[group("test")]
test-capability: (_run-test-kernel 'capability-test')

# Boot ppc-test: Invocation construction and same-Thread PPC Call/Return into the Bounce AddressSpace
[group("test")]
test-ppc: (_run-test-kernel 'ppc-test')

# Boot preempt-test: interrupt-controller component, kernel timer tick and round-robin preemption of Threads that never yield
[group("test")]
test-preempt: (_run-test-kernel 'preempt-test' 'build-preempt-test')

# Boot sync-test: Notification, EventCount, blocking waits through the Bounce Thread, Thread.Retire
[group("test")]
test-sync: (_run-test-kernel 'sync-test')

# Boot memory-test: PageTable/Frame mapping, alias policy, ASIDs, activation and TLB invalidation, AddressSpace.Retire
[group("test")]
test-memory: (_run-test-kernel 'memory-test')

# Boot endpoint-test: client, endpoint and server AddressSpaces rendezvous through PPC
[group("test")]
test-endpoint: (_run-test-kernel 'endpoint-test' 'build-endpoint-test')

# Boot fp-trap-test: an FP/SIMD instruction must trap at EL1t and at EL0
[group("test")]
test-fp-trap: (_run-test-kernel 'fp-trap-test' 'build-fp-trap-test')

# Boot fault-test: faults are delivered to EL0 fault handlers (skip, retry, terminate, Return faults) and every unhandled case parks the Thread
[group("test")]
test-fault: (_run-test-kernel 'fault-test' 'build-fault-test')

# Run chainboot tests in QEMU (rpi3) with its own linker script
[group("test")]
test-chainboot:
    RUSTFLAGS="{{ fixed_rustflags }} {{ board_rpi3_flags }} -C link-arg=--script={{ chainboot_link }}" \
    cargo test {{ target_json }} --features=qemu {{ rust_std }} \
      -p chainboot

# Run capability, boot-platform and host tool tests natively
[group("test")]
test-host: test-object-host test-platform-host
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
test-platform-host: (_host-test 'vesper-devicetree' 'devicetree') (_host-test 'vesper-image' 'privileged') (_host-test 'irqchip-bcm2836' 'bcm2836')

# Test runner invoked by .cargo/config.toml runner
[private]
_test-runner binary_path:
    #!/usr/bin/env bash
    set -euo pipefail
    name=$(basename "{{ binary_path }}")
    bin="{{ justfile_directory() }}/target/${name}.bin"
    {{ objcopy }} --strip-all -O binary "{{ binary_path }}" "${bin}"
    echo "🚨 Running test: ${name}"
    {{ qemu_test }} -kernel "${bin}"

# === Clippy ===

# Cross-clippy the workspace for a board with a feature set
[private]
_cross-clippy board='rpi3' features='':
    RUSTFLAGS="{{ fixed_rustflags }} {{ if board == 'rpi3' { board_rpi3_flags } else { board_rpi4_flags } }}" \
    cargo clippy {{ target_json }} \
      {{ if features != '' { '--features=' + features } else { '' } }} \
      {{ rust_std }} \
      --workspace --exclude=chainofcommand --exclude=vesper-image-build \
      -- --deny warnings --allow deprecated

# Run embedded clippy checks (all feature combos) and capability host-test linting
[group("maintenance")]
clippy: (build 'rpi3' 'qemu') (build-components 'rpi3' 'qemu') (_cross-clippy 'rpi3' '') (_cross-clippy 'rpi4' '') (_cross-clippy 'rpi3' 'noserial') (_cross-clippy 'rpi3' 'qemu') (_cross-clippy 'rpi3' 'noserial,qemu') (_cross-clippy 'rpi3' 'jtag') (_cross-clippy 'rpi3' 'noserial,jtag') (build 'rpi3' 'qemu,debug_kernel') (_cross-clippy 'rpi3' 'debug_kernel') (_cross-clippy 'rpi3' 'qemu,debug_kernel') (_cross-clippy 'rpi3' 'qemu,fp_trap_test') clippy-object-host clippy-platform-host

# Run shortened clippy (default features on both boards) and capability host-test linting
[group("maintenance")]
clippy-pre-push: (_cross-clippy 'rpi3' '') (_cross-clippy 'rpi4' '') clippy-object-host clippy-platform-host

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
clippy-platform-host: (_host-clippy 'vesper-devicetree' 'devicetree') (_host-clippy 'vesper-image' 'privileged') (_host-clippy 'irqchip-bcm2836' 'bcm2836')

# Clippy for the host tools (chainofcommand, vesper-image-build)
[private]
_clippy-coc:
    cargo clippy -p vesper-image-build -- --deny warnings --allow deprecated # FIXME Shouldn't be here!
    cargo clippy -p chainofcommand -- --deny warnings --allow deprecated

# Build and disassemble kernel
[group("debug")]
hopper: build
    hopper --loader ELF --executable "{{ kernel_elf }}"

alias disasm := hopper

[group("debug")]
cb-hopper: (build-chainboot 'rpi3' 'qemu')
    #hopper --loader RAW --plugin arm --cpu aarch64 --variant generic --base-address 0x80000 --executable "{{ chainboot_bin }}"

alias cb-disasm := cb-hopper

# === Maintenance & Tools ===

# Build and print all symbols
[group("maintenance")]
nm: build
    {{ nm }} "{{ kernel_elf }}" | sort -k 1 | rustfilt

# Run `cargo expand` on kernel
[group("maintenance")]
expand:
    cargo expand {{ target_json }} --release -- kernel

# Generate and open documentation
[group("maintenance")]
doc:
    cargo doc --open --no-deps {{ target_json }} {{ rust_std }}

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
audit-fp-simd: (_build-test-kernel 'capability-test') (_build-test-kernel 'memory-test') (_build-test-kernel 'sync-test') (_build-test-kernel 'ppc-test') (build-preempt-test 'rpi3') (build-fault-test 'rpi3') (build-endpoint-test 'rpi3' 'qemu')
    #!/usr/bin/env bash
    set -euo pipefail
    artifacts="{{ release_dir }}"
    # An instruction line whose mnemonic is an FP one (f…), or whose operands name an
    # FP/SIMD register (b/h/s/d/q/v0..31, with an optional arrangement) or FPCR/FPSR.
    fp_simd='^[[:space:]]+[0-9a-f]+:[[:space:]]+(f[a-z0-9]*|[a-z0-9.]+[[:space:]].*\b([bhsdqv][0-9]{1,2}(\.[0-9]*[bhsdq])?|fpcr|fpsr)\b)'
    failed=0
    # fp-probe executes an FP/SIMD instruction on purpose.
    user_components="{{ replace(user_components, 'fp-probe', '') }}"
    for image in nucleus kickstart {{ privileged_components }} capability-test memory-test sync-test ppc-test preempt-test endpoint-test fault-test ${user_components}; do
        # Symbol names in <…> are not operands.
        found=$(rust-objdump -d --no-show-raw-insn "${artifacts}/${image}" | sed 's/<[^>]*>//g' | grep -E "${fp_simd}" || true)
        if [ -n "${found}" ]; then
            echo "❌ ${image} contains FP/SIMD instructions:"
            echo "${found}"
            failed=1
        else
            echo "{{ ok_label }} ${image}: integer-only"
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
