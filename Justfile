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
test_link       := 'libs/platform/src/raspberrypi/linker/test.ld'
chainboot_link  := 'bin/chainboot/src/link.ld'

fixed_rustflags := '-D warnings -Z macro-backtrace'

qemu            := env('QEMU', 'qemu-system-aarch64')
qemu_machine    := env('QEMU_MACHINE', 'raspi3b')
gdb             := env('GDB', 'aarch64-elf-gdb') # An aarch64-enabled GDB (brew install aarch64-elf-gdb)
objcopy         := 'rust-objcopy'
nm              := 'rust-nm'
volume          := env('VOLUME', '/Volumes/BOOT')

kernel_elf      := justfile_directory() / 'target' / target / 'release/kickstart'
kernel_bin      := justfile_directory() / 'target/kernel.bin'
chainboot_elf   := justfile_directory() / 'target' / target / 'release/chainboot'
chainboot_bin   := justfile_directory() / 'target/chainboot.bin'

chainboot_serial := '/dev/tty.SLAB_USBtoUART'
chainboot_baud   := '115200'

# QEMU option fragments
qemu_base_opts    := '-M ' + qemu_machine + ' -chardev stdio,mux=on,id=char0,logfile=qemu.log,signal=off -object monitor-hmp,chardev=char0,id=mon0 -serial chardev:char0 -semihosting-config enable=on,chardev=char0'
qemu_disasm       := '-d in_asm,unimp,int,mmu,cpu_reset,guest_errors,nochain,plugin'
qemu_gdb_opts     := '-gdb tcp::5555 -S'
qemu_test_opts    := '-nographic'
qemu_disasm_gdb   := qemu_disasm + ' ' + qemu_gdb_opts

gdb_connect     := justfile_directory() / 'target' / target / 'gdb-connect'

openocd_bin     := env('OPENOCD', '/usr/local/opt/openocd/4d6519593-rtt/bin/openocd')

ok_label        := '✅'
copy_label      := '🔄'

_default:
    @just --list

# === Low-level: cross-compile a single crate ===

# Cross-build a crate for a given board with a given linker script and features
[private]
_cross-build crate board='rpi4' linker_script='' features='':
    RUSTFLAGS="{{ fixed_rustflags }} {{ if board == 'rpi3' { board_rpi3_flags } else { board_rpi4_flags } }}{{ if linker_script != '' { ' -C link-arg=--script=' + linker_script } else { '' } }}" \
    cargo build {{ target_json }} \
      {{ if features != '' { '--features=' + features } else { '' } }} \
      {{ rust_std }} \
      --release -p {{ crate }}

# === Kernel (nucleus + kickstart -> kernel.bin) ===

# Build kernel (features: '' for hw, 'qemu' for emulation)
[group("hw")]
build board='rpi4' features='': (_cross-build 'nucleus' board nucleus_link features) (_cross-build 'kickstart' board init_link features)
    {{ objcopy }} --strip-all -O binary "{{ kernel_elf }}" "{{ kernel_bin }}"
    @# TODO: print final binary size!
    @echo "{{ok_label}} kernel built for {{ board }}{{ if features != '' { ' [' + features + ']' } else { '' } }}"

alias b := build

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
[group("emu")]
test: test-device test-chainboot test-host

alias t := test

# Run device crate tests in QEMU (rpi3) --verbose
[group("emu")]
test-device:
    RUSTFLAGS="{{ fixed_rustflags }} {{ board_rpi3_flags }} -C link-arg=--script={{ test_link }}" \
    cargo test --tests {{ target_json }} --features=qemu {{ rust_std }} \
      --workspace --exclude=chainofcommand --exclude=chainboot

    RUSTFLAGS="{{ fixed_rustflags }} {{ board_rpi3_flags }} -C link-arg=--script={{ test_link }}" \
    cargo test --doc {{ target_json }} --features=qemu {{ rust_std }} \
    --workspace --exclude=chainofcommand --exclude=chainboot

# Run chainboot tests in QEMU (rpi3) with its own linker script
[group("emu")]
test-chainboot:
    RUSTFLAGS="{{ fixed_rustflags }} {{ board_rpi3_flags }} -C link-arg=--script={{ chainboot_link }}" \
    cargo test {{ target_json }} --features=qemu {{ rust_std }} \
      -p chainboot

# Run capability and host tool tests natively
[group("emu")]
test-host: test-object-host
    cargo test -p chainofcommand

# Run the opt-in capability ABI tests on the native host (currently AArch64)
[group("emu")]
test-object-host:
    RUSTFLAGS="{{ fixed_rustflags }}" \
    cargo test -p vesper-objects --features=host-tests --test object_type

# Test runner invoked by .cargo/config.toml runner
[private]
_test-runner binary_path:
    #!/usr/bin/env bash
    set -euo pipefail
    name=$(basename "{{ binary_path }}")
    bin="{{ justfile_directory() }}/target/${name}.bin"
    {{ objcopy }} --strip-all -O binary "{{ binary_path }}" "${bin}"
    echo "🚨 Running test: ${name}"
    {{ qemu }} {{ qemu_base_opts }} {{ qemu_test_opts }} -dtb "{{ rpi3_dtb }}" -kernel "${bin}"

# === Clippy ===

# Cross-clippy a single feature set
[private]
_clippy-cross features='' board='rpi3':
    RUSTFLAGS="{{ fixed_rustflags }} {{ if board == 'rpi3' { board_rpi3_flags } else { board_rpi4_flags } }}" \
    cargo clippy {{ target_json }} \
      {{ if features != '' { '--features=' + features } else { '' } }} \
      {{ rust_std }} \
      --workspace --exclude=chainofcommand \
      -- --deny warnings --allow deprecated

# Run embedded clippy checks (all feature combos) and capability host-test linting
[group("maintenance")]
clippy: (build 'rpi3' 'qemu') (_clippy-cross '' 'rpi3') (_clippy-cross '' 'rpi4') (_clippy-cross 'noserial' 'rpi3') (_clippy-cross 'qemu' 'rpi3') (_clippy-cross 'noserial,qemu' 'rpi3') (_clippy-cross 'jtag' 'rpi3') (_clippy-cross 'noserial,jtag' 'rpi3') clippy-object-host

# Run shortened clippy (default features on both boards) and capability host-test linting
[group("maintenance")]
clippy-pre-push: (_clippy-cross '' 'rpi3') (_clippy-cross '' 'rpi4') clippy-object-host

# Lint the opt-in capability ABI tests with their native host harness
[group("maintenance")]
clippy-object-host:
    RUSTFLAGS="{{ fixed_rustflags }}" \
    cargo clippy -p vesper-objects --features=host-tests --test object_type \
      -- --deny warnings --allow deprecated

# Clippy for chainofcommand (host tool)
[private]
_clippy-coc:
    cargo clippy -p chainofcommand -- --deny warnings --allow deprecated

# === Maintenance & Tools ===

# Build and disassemble kernel
[group("debug")]
hopper: build
    hopper --loader ELF --executable "{{ kernel_elf }}"

alias disasm := hopper

[group("debug")]
cb-hopper: (build-chainboot 'rpi3' 'qemu')
    #hopper --loader RAW --plugin arm --cpu aarch64 --variant generic --base-address 0x80000 --executable "{{ chainboot_bin }}"

alias cb-disasm := cb-hopper

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

# Run lint tasks
[group("maintenance")]
lint: fmt-check clippy _clippy-coc

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
