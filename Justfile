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

test_link       := 'linker/test.ld'
chainboot_link  := 'bin/chainboot/src/link.ld'

fixed_rustflags := '-D warnings -Z macro-backtrace'
# Device tests run in QEMU (rpi3) with the test linker script
device_test_rustflags := fixed_rustflags + ' ' + board_rpi3_flags + ' -C link-arg=--script=' + test_link

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
rpi4_hw         := 'rpi4' + profile_suffix
# Boot images and ELFs of the configurations the run, debug and SD-card recipes use
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

# === QEMU runners ===

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

# Build and run chainboot in GDB
[group("debug")]
cb-gdb: build-chainboot _write-gdb-config
    {{ gdb }} -x "{{ gdb_connect }}" "{{ chainboot_rpi4_elf }}"

# Build and disassemble chainboot
[group("debug")]
cb-hopper: (build-chainboot 'rpi3' 'qemu')
    #hopper --loader RAW --plugin arm --cpu aarch64 --variant generic --base-address 0x80000 --executable "{{ chainboot_rpi3_qemu_bin }}"

alias cb-disasm := cb-hopper

# Run `cargo expand` on kernel
[group("maintenance")]
expand:
    cargo expand {{ target_json }} --release -- kernel

# === Hardware: SD card, serial boot, JTAG ===

# Build and write chainboot to SD Card, then eject
[group("hw")]
cb-eject board='rpi4': (build-chainboot board)
    cp "{{ images_dir / 'chainboot-' + board + '.bin' }}" "{{ volume }}/{{ 'chainboot-' + board + '.bin' }}"
    @echo "{{copy_label}} copied chainboot to {{ volume }}/{{ 'chainboot-' + board + '.bin' }}"
    # TODO: sed proper kernel name here in config.txt
    diskutil unmountDisk "{{ volume }}"

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

# The device tests in QEMU (rpi3), chainboot's, and the host tests.
# Run every test
[group("test")]
test: _test-device _test-chainboot test-host

alias t := test

# Run device crate tests in QEMU (rpi3) --verbose
_test-device:
    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ device_test_rustflags }}" \
    cargo test --tests {{ target_json }} --features=qemu {{ rust_std }} \
      --workspace --exclude=chainofcommand --exclude=chainboot

    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ device_test_rustflags }}" \
    cargo test --doc {{ target_json }} --features=qemu {{ rust_std }} \
    --workspace --exclude=chainofcommand --exclude=chainboot --exclude=vesper-boot-uart

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

# The boot platform, the chainboot protocol and chainofcommand.
# Run the host-side tests natively
[group("test")]
test-host: _test-platform-host _test-chainboot-host

# Run one opt-in (`host-tests`) integration test natively (currently AArch64 hosts)
_host-test package test features='host-tests':
    RUSTFLAGS="{{ fixed_rustflags }}" \
    cargo test -p {{ package }} --features={{ features }} --test {{ test }}

# Run the boot-platform tests natively: device tree helpers against the board
# DTBs in targets/
_test-platform-host: (_host-test 'vesper-devicetree' 'devicetree')

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
      --workspace --exclude=chainofcommand \
      -- --deny warnings --allow deprecated

# Run embedded clippy checks (all feature combos) and the host-test linting
[group("lint")]
clippy: (_cross-clippy 'rpi3' '') (_cross-clippy 'rpi4' '') (_cross-clippy 'rpi3' 'noserial') (_cross-clippy 'rpi3' 'qemu') (_cross-clippy 'rpi3' 'noserial,qemu') (_cross-clippy 'rpi3' 'jtag') (_cross-clippy 'rpi3' 'noserial,jtag') (_cross-clippy 'rpi3' 'mmu') _clippy-host

# Run shortened clippy (default features on both boards) and the host-test linting
_clippy-pre-push: (_cross-clippy 'rpi3' '') (_cross-clippy 'rpi4' '') _clippy-host

# Lint every host test and host tool
_clippy-host: _clippy-platform-host _clippy-coc

# Lint one opt-in (`host-tests`) integration test with its native host harness
_host-clippy package test features='host-tests':
    RUSTFLAGS="{{ fixed_rustflags }}" \
    cargo clippy -p {{ package }} --features={{ features }} --test {{ test }} \
      -- --deny warnings --allow deprecated

# Lint the boot-platform host tests
_clippy-platform-host: (_host-clippy 'vesper-devicetree' 'devicetree')

# Clippy for the host tools (chainofcommand and its protocol crate's host tests)
_clippy-coc: (_host-clippy 'vesper-chainboot-protocol' 'chainboot_protocol')
    cargo clippy -p chainofcommand --all-targets -- --deny warnings --allow deprecated

# Check formatting
[group("maintenance")]
fmt-check:
    cargo +nightly fmt -- --check

# Run every lint: formatting, clippy for every feature set and the host tests
[group("lint")]
lint: fmt-check clippy

# === CI ===

# Run pre-push local checks
[group("ci")]
pre-push: fmt-check _clippy-pre-push test

# Run CI tasks
[group("ci")]
ci: clean lint test

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

# Prepare local dev tools and set-up git hooks
[group("maintenance")]
setup-local-dev:
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
