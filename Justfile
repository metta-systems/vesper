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
# The rpi3 tree with the PWM node enabled for the headphone jack (`_audio-test-dtb` makes it
# from targets/overlays/audio-test-pwm.dts, as the board's `pwm-2chan` overlay does)
audio_test_dtb    := justfile_directory() / 'target/audio-test.dtb'

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
# The SC-55 bank trimmed to the shareware IWAD's music (`wood-soundfont`, audio decision D9.4)
wood_soundfont      := target_root / 'wood' / 'sc55-doom1.sf2'
# `bytes:looping:hash` of the songs `test-wood` hears first (the title's D_INTROA once, the
# first demo's D_E1M5 looping, at volume 64): the hash of their first 64 rendered periods as
# the MIDI synthesiser logs it and its host test computes it (midi-synth tests/session.rs)
wood_music_hashes   := '878:false:0x40f16e7eae7a5f45 13101:true:0x4f0a9ddf3a4a14be'
# Doom's E1M1 music as a Standard MIDI File, the song midi-test plays (`wood-midi`)
wood_midi           := target_root / 'wood' / 'd_e1m1.mid'
# `bytes:looping:hash` of the song `test-midi` plays (D_E1M1 looping, at volume 64), as above
# (`start_hash_of_the_midi_test_song` in midi-synth tests/session.rs)
midi_test_hash      := '23329:true:0x4366085f6c416218'

chainboot_serial    := '/dev/ttyUSB0' # '/dev/cu.usbserial-019586E3'
chainboot_baud      := '115200'
# The rate chainofcommand asks chainboot to load the kernel at, and the console stays at
# (e.g. `just chainboot_load_baud=2000000 boot-wood-test`); empty loads at `chainboot_baud`
chainboot_load_baud := '1500000'

chainofcommand_args := chainboot_serial + ' ' + chainboot_baud + (if chainboot_load_baud != '' { ' --load-baud ' + chainboot_load_baud } else { '' })

# QEMU option fragments
# Console, monitor and semihosting wiring shared by every QEMU run
qemu_stdio_opts   := '-chardev stdio,mux=on,id=char0,logfile=qemu.log,signal=off -object monitor-hmp,chardev=char0,id=mon0 -semihosting-config enable=on,userspace=on,chardev=char0'
qemu_io_opts      := '-serial chardev:char0 ' + qemu_stdio_opts
qemu_base_opts    := '-M ' + qemu_machine + ' ' + qemu_io_opts
qemu_disasm       := '-d in_asm,unimp,int,mmu,cpu_reset,guest_errors,nochain,plugin'
qemu_gdb_opts     := '-gdb tcp::5555 -S'
qemu_test_opts    := '-nographic'
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
[private]
build-privileged board='rpi4' image_features='': (_cross-build privileged_components board privileged_link '' pie_rustflags image_features)

# Build the EL0 driver components the kickstart image bundles (with semihosting output under 'qemu')
[private]
build-drivers board='rpi4' features='' image_features=features:
    {{ just_executable() }} _cross-build '{{ driver_components }}' '{{ board }}' '{{ user_link }}' '{{ if features =~ "qemu" { "qemu" } else { "" } }}' '' '{{ image_features }}'

# Build kernel (features: '' for hw, 'qemu' for emulation) into target/images/kernel-<config>.bin
[private]
build board='rpi4' features='' config=(board + (if features == '' { '' } else { '-' + replace(features, ',', '+') }) + profile_suffix): (_cross-build 'nucleus' board nucleus_link features) (build-privileged board features) (build-drivers board features) (_cross-build 'kickstart' board init_link features)
    @mkdir -p "{{ images_dir }}"
    {{ objcopy }} --strip-all -O binary "{{ target_root / config / target / image_profile / 'kickstart' }}" "{{ images_dir / 'kernel-' + config + '.bin' }}"
    @# TODO: print final binary size!
    @echo "{{ok_label}} kernel built for {{ board }}{{ if features != '' { ' [' + features + ']' } else { '' } }}: {{ images_dir / 'kernel-' + config + '.bin' }}"

alias b := build

# Build the EL0 userspace components (linked with the userspace runtime's user.ld) for the
# image configuration `board` + `image_features`
[private]
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
    {{ qemu }} -M raspi3b {{ qemu_io_opts }} {{ qemu_test_opts }} {{ qemu_usb }} \
      -dtb "{{ rpi3_dtb }}" -kernel "{{ images_dir / 'usb-test-' + config + '.bin' }}"

# Build the sd-test e2e kernel (the SDHOST component and a program reading and writing the card through block_v1)
[group("test")]
build-sd-test board='rpi3': (build-components board 'qemu' test_kernel_features) (_build-test-kernel 'sd-test' board)

# Boot sd-test on raspi3b with a fresh 4 MiB card image whose blocks 0, 5, 6 and 8191 start with a marker: the program reads them, writes a block back and checks the request rules
[group("test")]
test-sd board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (build-sd-test board)
    #!/usr/bin/env bash
    set -euo pipefail
    image="{{ target_root / 'sd-test.img' }}"
    dd if=/dev/zero of="${image}" bs=1048576 count=4 2>/dev/null
    for block in 0 5 6 8191; do
        printf 'vesper sd block %d' "${block}" | dd of="${image}" bs=512 seek="${block}" conv=notrunc 2>/dev/null
    done
    {{ qemu }} -M raspi3b {{ qemu_io_opts }} {{ qemu_test_opts }} -drive if=sd,format=raw,file="${image}" \
      -dtb "{{ rpi3_dtb }}" -kernel "{{ images_dir / 'sd-test-' + config + '.bin' }}"

# Build the audio-test e2e kernel (the clock manager component, the DMA controller component, the PWM audio driver, the gpio component and the PL011 driver)
[group("test")]
build-audio-test board='rpi3': (build-components board 'qemu' test_kernel_features) (_build-test-kernel 'audio-test' board)

# Make the audio test tree: the rpi3 tree with targets/overlays/audio-test-pwm.dts applied, which enables the PWM node with the jack's pins as the board's `dtoverlay=pwm-2chan,pin=40,func=4,pin2=41,func2=4` does (needs dtc and fdtoverlay)
[private]
_audio-test-dtb:
    @mkdir -p "{{ target_root }}"
    dtc -q -@ -I dts -O dtb -o "{{ target_root / 'audio-test-pwm.dtbo' }}" "{{ justfile_directory() / 'targets/overlays/audio-test-pwm.dts' }}"
    fdtoverlay -i "{{ rpi3_dtb }}" -o "{{ audio_test_dtb }}" "{{ target_root / 'audio-test-pwm.dtbo' }}"

# Boot audio-test on raspi3b with the audio test tree: the audio driver sets the PWM clock to 100 MHz through the clock manager component and reads it back, and clock_v1 refuses an unknown clock and an unreachable rate; then the test client runs the capture test through audio_v1: QEMU has no PWM block and cannot run a DMA ring, so the driver plays each period by a DMA copy into a capture buffer and checks the captured words against the client's pattern (converted, in order, silence and underruns once the client stops), and packets come back in order (`just boot-audio-test` plays a tone on the board). The console goes to target/audio-test.log
[group("test")]
test-audio board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (build-audio-test board) _audio-test-dtb
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

# Boot audio-test on raspi3b for its DMA checks: Kickstart hands the DMA controller component each interrupt line once, the audio driver copies memory through dma_v1 with chains of linked control blocks on the first channel and on one sharing line 27, one completion per block, and dma_v1 refuses what it must. QEMU's DMA model runs a chain inside the write that starts it, so no ring runs here (`just boot-audio-test` plays one on the board). The console goes to target/dma-test.log
[group("test")]
test-dma board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (build-audio-test board) _audio-test-dtb
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

# Build audio-test for the Raspberry Pi 3 hardware (no QEMU, no semihosting): the test client streams a tone through audio_v1 to the audio driver's DMA ring, which traces each stream on the UART
[group("hw")]
build-audio-test-hw board='rpi3': (build-components board '' '') (_build-test-kernel 'audio-test' board '')

# Build audio-test for the Raspberry Pi 3 and boot it via chainofcommand: with `dtoverlay=pwm-2chan,pin=40,func=4,pin2=41,func2=4` in config.txt, the test client plays a 440 Hz tone through audio_v1 on the left side of the headphone jack for 3 s, the right for 3 s, both for 3 s, then both for a minute, 0.7 s apart; the audio driver traces on the UART, at each close, the measured sample rate, the underruns, late periods, the PWM's STA flags and the DMA errors
[group("hw")]
boot-audio-test: build-audio-test-hw chainofcommand
    target/debug/chainofcommand {{ chainofcommand_args }} --kernel "{{ images_dir / 'audio-test-rpi3.bin' }}"

# Build the usb-core-test e2e kernel (the DWC2 component, the PL011 driver and the USB core, loaded by Kickstart)
[group("test")]
build-usb-core-test board='rpi3': (build-components board 'qemu' test_kernel_features) (_build-test-kernel 'usb-core-test' board)

# Boot usb-core-test on raspi3b with a hub on the root port and a keyboard behind it: the USB core enumerates both
[group("test")]
test-usb-core board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (build-usb-core-test board)
    {{ qemu }} -M raspi3b {{ qemu_io_opts }} {{ qemu_test_opts }} {{ qemu_usb }} \
      -dtb "{{ rpi3_dtb }}" -kernel "{{ images_dir / 'usb-core-test-' + config + '.bin' }}"

# Build usb-core-test for the Raspberry Pi 3 hardware (no QEMU, no semihosting): `usb-core` traces on the UART
[group("hw")]
build-usb-core-test-hw board='rpi3': (build-components board '' '') (_build-test-kernel 'usb-core-test' board '')

# Build usb-core-test for the Raspberry Pi 3 and boot it via chainofcommand: `usb-core`'s trace appears on the UART
[group("hw")]
boot-usb-core-test: build-usb-core-test-hw chainofcommand
    target/debug/chainofcommand {{ chainofcommand_args }} --kernel "{{ images_dir / 'usb-core-test-rpi3.bin' }}"

# Build the usb-hid-test e2e kernel (the DWC2 component, the PL011 driver, the USB core, the HID component and the input-trace program)
[group("test")]
build-usb-hid-test board='rpi3': (build-components board 'qemu' test_kernel_features) (_build-test-kernel 'usb-hid-test' board)

# Boot usb-hid-test on raspi3b with a hub on the root port and a keyboard behind it: the HID component claims the keyboard and input-trace reads it through input_v1
[group("test")]
test-usb-hid board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (build-usb-hid-test board)
    {{ qemu }} -M raspi3b {{ qemu_io_opts }} {{ qemu_test_opts }} {{ qemu_usb }} \
      -dtb "{{ rpi3_dtb }}" -kernel "{{ images_dir / 'usb-hid-test-' + config + '.bin' }}"

# TEMP: Test the hw kernel in qemu with passthrough USB kb and mouse
[group("test")]
test-usb-hid-hw:
    #sudo {{ qemu }} -M raspi3b {{ qemu_io_opts }} {{ qemu_test_opts }} {{ qemu_usb_forward }} \
    #  -dtb "{{ rpi3_dtb }}" -kernel "{{ images_dir / 'usb-hid-test-rpi3.bin' }}"

# Build usb-hid-test for the Raspberry Pi 3 hardware (no QEMU, no semihosting): `input-trace` prints the keyboard's events on the UART
[group("hw")]
build-usb-hid-test-hw board='rpi3': (build-components board '' '') (_build-test-kernel 'usb-hid-test' board '')

# Build usb-hid-test for the Raspberry Pi 3 and boot it via chainofcommand: plug in a keyboard and press keys, `input-trace` prints the events on the UART
[group("hw")]
boot-usb-hid-test: build-usb-hid-test-hw chainofcommand
    target/debug/chainofcommand {{ chainofcommand_args }} --kernel "{{ images_dir / 'usb-hid-test-rpi3.bin' }}"

# Build the compositor-test e2e kernel (the DWC2 component, the PL011 driver, the USB core, the HID component and the compositor)
[group("test")]
build-compositor-test board='rpi3': (build-components board 'qemu' test_kernel_features) (_build-test-kernel 'compositor-test' board)

# Boot compositor-test on raspi3b with a hub on the root port and a keyboard behind it: the compositor reads the keyboard through input_v1 and has drawn the background on the framebuffer
[group("test")]
test-compositor board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (build-compositor-test board)
    @rm -f target/compositor-test.log
    {{ qemu }} -M raspi3b {{ qemu_io_opts }} {{ qemu_test_opts }} -device usb-hub,port=1 -device usb-kbd,port=1.1 \
      -dtb "{{ rpi3_dtb }}" -kernel "{{ images_dir / 'compositor-test-' + config + '.bin' }}" > target/compositor-test.log
    @grep -q "compositor: frame 1 drew the background: true" target/compositor-test.log || { echo "compositor-test: the background did not reach the framebuffer"; exit 1; }
    @echo "{{ ok_label }} the compositor drew the background and read the keyboard"

# Build compositor-test for the Raspberry Pi 3 hardware (no QEMU, no semihosting): the compositor traces the routed input on the UART
[group("hw")]
build-compositor-test-hw board='rpi3': (build-components board '' '') (_build-test-kernel 'compositor-test' board '')

# Build compositor-test for the Raspberry Pi 3 and boot it via chainofcommand: plug in a mouse and a keyboard, the compositor traces the routed input on the UART
[group("hw")]
boot-compositor-test: build-compositor-test-hw chainofcommand
    target/debug/chainofcommand {{ chainofcommand_args }} --kernel "{{ images_dir / 'compositor-test-rpi3.bin' }}"

# Build wood-test for the Raspberry Pi 3 hardware (no QEMU, no semihosting): the compositor with the lava lamp and Wood playing Doom in a window, keyboard and mouse over USB, its sound effects and music through the PWM audio driver on the headphone jack; the log server writes the lines of the compositor (the routed input), Wood (with a music summary every 256 periods), the MIDI synthesiser, the audio driver and the USB components (controller bring-up, port resets, enumeration, the keyboard served) on the UART
[group("hw")]
build-wood-test-hw board='rpi3': (build-components board '' '') (build-wood board '' '') (_build-test-kernel 'wood-test' board '')

# Build wood-test for the Raspberry Pi 3 and boot it via chainofcommand: plug in a keyboard (and a mouse to move focus between windows), and headphones in the jack: with `dtoverlay=pwm-2chan,pin=40,func=4,pin2=41,func2=4` in config.txt the audio driver loads and Wood plays Doom's sound effects (without it Wood runs silent)
[group("hw")]
boot-wood-test: build-wood-test-hw chainofcommand
    target/debug/chainofcommand {{ chainofcommand_args }} --kernel "{{ images_dir / 'wood-test-rpi3.bin' }}"

# Build Wood, the Room Doom engine's Vesper program, the MIDI synthesiser component that plays
# its music, with the trimmed SoundFont it bundles, and the log server that writes their lines to
# the UART, for the image configuration `board` + `image_features`; only wood-test bundles them,
# so they are not in `user_components`
[private]
build-wood board='rpi3' features='qemu' image_features=features: wood-soundfont (_cross-build 'wood midi-synth log-server' board user_link features '' image_features)

# Build the wood-test e2e kernel: compositor-test's components plus Wood, the Room Doom engine in a window of its own
[group("test")]
build-wood-test board='rpi3': (build-components board 'qemu' test_kernel_features) (build-wood board 'qemu' test_kernel_features) (_build-test-kernel 'wood-test' board)

# Boot wood-test on raspi3b with the devices of test-compositor and the audio test tree (`_audio-test-dtb`, the PWM node enabled), wait up to `seconds` for the engine to start, press Escape (Doom's menu opens) and Down through the QEMU monitor, save the screen and stop QEMU: Wood's window has the focus as the newest window, so the keys reach it, and draw-demo's lava lamp, whose window lost the focus to it, must pause. Wood's playback Thread pulls each period's music from the MIDI synthesiser (midi_synth_v1), mixes the demo's sound effects on top and streams the result through audio_v1; the songs' first periods must hash as on the host (`wood_music_hashes`); the audio driver plays each period into its capture buffer by DMA copies, checks it against the ring and counts the audible ones. The log server holds the UART: the lines of the compositor, Wood, the synthesiser, the audio driver and the USB components (the controller's bring-up and port reset, a device the core enumerated, the keyboard the HID component serves) must reach it with its timestamp and level prefix, and Wood's music summary must appear. The semihosting console goes to target/wood-test.log, the UART to target/wood-test-serial.log, the screen to target/wood-test.ppm
[group("test")]
test-wood board='rpi3' seconds='90' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (build-wood-test board) _audio-test-dtb
    #!/usr/bin/env bash
    set -uo pipefail
    log="{{ target_root }}/wood-test.log"
    serial="{{ target_root }}/wood-test-serial.log"
    screen="{{ target_root }}/wood-test.ppm"
    monitor="{{ target_root }}/wood-test-monitor.sock"
    rm -f "$log" "$serial" "$screen" "$monitor"
    {{ qemu }} -M raspi3b -display none -monitor "unix:$monitor,server,nowait" \
      -chardev stdio,mux=on,id=char0,signal=off -serial "file:$serial" -semihosting-config enable=on,userspace=on,chardev=char0 \
      -device usb-hub,port=1 -device usb-kbd,port=1.1 \
      -dtb "{{ audio_test_dtb }}" -kernel "{{ images_dir / 'wood-test-' + config + '.bin' }}" < /dev/null > "$log" &
    qemu_pid=$!
    monitor_command() { printf '%s\n' "$1" | nc -N -U "$monitor" > /dev/null; }
    # The engine runs and the keyboard is the compositor's before keys are pressed.
    for _ in $(seq {{ seconds }}); do
      grep -q "wood: INFO the engine started" "$log" && grep -q "compositor: device 1:" "$log" && break
      sleep 1
    done
    sleep 5
    monitor_command "sendkey esc"
    sleep 2
    monitor_command "sendkey down"
    sleep 3
    monitor_command "screendump $screen"
    sleep 2
    monitor_command "quit"
    wait "$qemu_pid"
    grep -q "compositor: frame 1 drew the background: true" "$log" || { echo "wood-test: the background did not reach the framebuffer"; exit 1; }
    grep -q "wood: INFO frame 1 presented: true" "$log" || { echo "wood-test: Wood presented no frame"; exit 1; }
    grep -q "wood: INFO focus gained" "$log" || { echo "wood-test: Wood's window did not get the focus"; exit 1; }
    grep -q "lamp: paused after frame [0-9]*, unfocused" "$log" || { echo "wood-test: the lava lamp kept animating without the focus"; exit 1; }
    ! grep -q "lamp: resumed" "$log" || { echo "wood-test: the lava lamp resumed while Wood had the focus"; exit 1; }
    grep -q "wood: INFO key 0x29 down: doom key 0x1b" "$log" || { echo "wood-test: Escape did not reach Wood"; exit 1; }
    grep -q "wood: INFO key 0x51 down: doom key 0xaf" "$log" || { echo "wood-test: Down did not reach Wood"; exit 1; }
    grep -a "wood: INFO audio: \|audio: open for\|audio: capture: " "$log" | tail -4
    grep -q "wood: INFO audio: open at 44091.710 Hz" "$log" || { echo "wood-test: Wood did not open the audio output"; exit 1; }
    grep -q "audio: open for client 0: a ramp up, then its packets" "$log" || { echo "wood-test: the audio driver did not start Wood's stream"; exit 1; }
    grep -q "wood: INFO audio: [0-9]* buffers mixed, [1-9][0-9]* with sound" "$log" || { echo "wood-test: Wood mixed no sound effect"; exit 1; }
    grep -q "audio: capture: [1-9][0-9]* audible stream periods of" "$log" || { echo "wood-test: no sound effect reached the capture buffer"; exit 1; }
    audible=$(grep -a -o "audio: capture: [0-9]* audible stream periods of [0-9]*" "$log" | tail -1 | cut -d' ' -f3,8)
    underruns=$(grep -a -o "with sound, [0-9]* underruns" "$log" | tail -1 | cut -d' ' -f3)
    # Music: the synthesiser loaded its SoundFont, Wood took it, the title song's first
    # periods hash as the host test computes them (`start_hashes_of_the_first_songs` in
    # userspace/wood/midi-synth/tests/session.rs), and nearly every period is audible.
    grep -a "midi-synth: \|wood: INFO music: " "$log" | tail -4
    grep -q "midi-synth: ready at 44091710 mHz" "$log" || { echo "wood-test: the MIDI synthesiser did not start"; exit 1; }
    grep -q "wood: INFO music: the MIDI synthesiser plays at 44091710 mHz" "$log" || { echo "wood-test: Wood did not take the MIDI synthesiser"; exit 1; }
    for expected in {{ wood_music_hashes }}; do
      bytes=${expected%%:*}; rest=${expected#*:}; looping=${rest%%:*}; hash=${rest#*:}
      line=$(grep -a -o "midi-synth: song of $bytes bytes (looping $looping, volume 64): first 64 periods hash 0x[0-9a-f]*, [0-9]* audible" "$log" | head -1)
      [ -n "$line" ] || { echo "wood-test: no start hash for the song of $bytes bytes"; exit 1; }
      [ "$(echo "$line" | grep -o '0x[0-9a-f]*')" = "$hash" ] || { echo "wood-test: the song of $bytes bytes rendered differently from the host: $line"; exit 1; }
    done
    music=$(grep -a -o "music: [0-9]* periods pulled, [0-9]* with sound" "$log" | tail -1 | cut -d' ' -f2,5)
    [ -n "$music" ] && [ $(( ${music#* } * 10 )) -ge $(( ${music% *} * 9 )) ] || { echo "wood-test: too few music periods with sound: $music"; exit 1; }
    [ $(( ${audible% *} * 10 )) -ge $(( ${audible#* } * 9 )) ] || { echo "wood-test: too few audible stream periods: $audible"; exit 1; }
    # The log server wrote every client's lines to the UART, each with its prefix.
    prefix='^\[ *[0-9]*\.[0-9]\{6\}\] I '
    grep -a "${prefix}wood: music: summary" "$serial" | tail -2
    for expected in "log-server: 7 clients, serial output attached" "compositor: display [0-9]*x[0-9]* pitch" "compositor: device 1 added" "wood: the engine started" "wood: music: summary of [0-9]* periods" "midi-synth: ready at 44091710 mHz" "audio: open for client 0" "usb-hcd-dwc2: core 0x[0-9a-f]* reset, host mode" "usb-hcd-dwc2: port reset: enabled" "usb-core: hub [0-9]* port [0-9]*: [a-z]* speed device at address [0-9]*" "usb-hid: serving a Keyboard as device"; do
      grep -a -q "${prefix}\(([0-9]* dropped) \)\?${expected}" "$serial" || { echo "wood-test: the UART got no line \"$expected\" from the log server"; exit 1; }
    done
    dropped=0
    for count in $(grep -a -o "([0-9]* dropped)" "$serial" | tr -dc '0-9\n'); do dropped=$(( dropped + count )); done
    usb_lines=$(grep -a -c "${prefix}\(([0-9]* dropped) \)\?usb" "$serial")
    echo "{{ ok_label }} Wood presented frames in its window and received Escape and Down; its music and sound effects reached the audio driver's capture (at least ${audible% *} of ${audible#* } stream periods audible, ${music#* } of ${music% *} music periods with sound, the songs' first periods as on the host, $underruns underruns); the log server wrote the compositor's, Wood's, the synthesiser's, the audio driver's and the USB components' lines to the UART ($usb_lines USB lines, $dropped dropped); the screen is in $screen"

# Build midi-test's programs: the player, the MIDI synthesiser and the log server, with the trimmed SoundFont and the song they bundle, for the image configuration `board` + `image_features`; only midi-test bundles the player, so it is not in `user_components`
[private]
build-midi-player board='rpi3' features='qemu' image_features=features: wood-soundfont wood-midi (_cross-build 'midi-player midi-synth log-server' board user_link features '' image_features)

# Build the midi-test e2e kernel: the MIDI synthesiser, a player program with Doom's D_E1M1, audio-test's drivers, the mailbox, the PL011 and the log server; no Wood, compositor or USB
[group("test")]
build-midi-test board='rpi3': (build-components board 'qemu' test_kernel_features) (build-midi-player board 'qemu' test_kernel_features) (_build-test-kernel 'midi-test' board)

# Boot midi-test on raspi3b with the audio test tree (`_audio-test-dtb`, the PWM node enabled): the player sends D_E1M1 to the MIDI synthesiser (looping, volume 64), pulls 512 periods with `service` and streams them through audio_v1, where the audio driver plays each into its capture buffer by DMA copies, checks it against the ring and counts the audible ones; then it closes and reports. The song must load and play, its first periods must hash as on the host (`midi_test_hash`), nearly every period must be audible, with 0 underruns, and the log server must write the player's summary to the UART. The semihosting console goes to target/midi-test.log, the UART to target/midi-test-serial.log
[group("test")]
test-midi board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (build-midi-test board) _audio-test-dtb
    #!/usr/bin/env bash
    set -uo pipefail
    log="{{ target_root }}/midi-test.log"
    serial="{{ target_root }}/midi-test-serial.log"
    rm -f "$log" "$serial"
    timeout 600 {{ qemu }} -M raspi3b -display none -monitor none \
      -chardev stdio,mux=on,id=char0,signal=off -serial "file:$serial" -semihosting-config enable=on,userspace=on,chardev=char0 \
      -dtb "{{ audio_test_dtb }}" -kernel "{{ images_dir / 'midi-test-' + config + '.bin' }}" < /dev/null > "$log"
    status=$?
    grep -a "midi-test: \|midi-player: \|midi-synth: \|audio: open for\|audio: closed\|audio: capture checked" "$log" | grep -v "midi-test: .* on " | tail -40
    [ "$status" -eq 0 ] || { echo "midi-test: failed with status $status; see $log"; exit 1; }
    expect() { grep -a -q "$1" "$log" || { echo "midi-test: $2; see $log"; exit 1; }; }
    expect "midi-synth: ready at 44091710 mHz" "the MIDI synthesiser did not start"
    expect "midi-player: INFO song: 23329 bytes, division 70 ticks per quarter, tempo 500000 us per quarter (120 bpm)" "the player did not read its song"
    expect "midi-player: INFO song: first notes: ch" "the player did not log the first notes"
    expect "midi-synth: a song of 23329 bytes plays" "the synthesiser did not take the song"
    expect "midi-player: INFO the synthesiser handled [1-9][0-9]* song packets; playing true at volume 64" "the synthesiser does not play the song"
    expect "audio: open for client 0: a ramp up, then its packets" "the audio driver did not start the stream"
    expect "midi-player: INFO music: summary of 256 periods: service min/avg/max" "no music summary"
    expect "midi-player: INFO music: level peak [1-9][0-9]* rms [1-9][0-9]* of 32767; voices peak [1-9][0-9]* of 32" "no level and voices line"
    expect "midi-player: INFO position: pass 0, tick [1-9][0-9]* of 13440" "no position line"
    # The song's first periods hash as the host test computes them
    # (`start_hash_of_the_midi_test_song` in userspace/wood/midi-synth/tests/session.rs).
    midi_hash="{{ midi_test_hash }}"
    bytes=${midi_hash%%:*}
    rest=${midi_hash#*:}; looping=${rest%%:*}; hash=${rest#*:}
    line=$(grep -a -o "midi-synth: song of $bytes bytes (looping $looping, volume 64): first 64 periods hash 0x[0-9a-f]*, [0-9]* audible" "$log" | head -1)
    [ -n "$line" ] || { echo "midi-test: no start hash for the song"; exit 1; }
    [ "$(echo "$line" | grep -o '0x[0-9a-f]*')" = "$hash" ] || { echo "midi-test: the song rendered differently from the host: $line"; exit 1; }
    # Nearly every period has sound, every one reached the capture, none late.
    closed=$(grep -a -o "midi-player: INFO closed after [0-9]* periods, [0-9]* with sound, [0-9]* underruns" "$log" | tail -1)
    [ -n "$closed" ] || { echo "midi-test: the player did not close"; exit 1; }
    periods=$(echo "$closed" | cut -d' ' -f5); sounding=$(echo "$closed" | cut -d' ' -f7); underruns=$(echo "$closed" | cut -d' ' -f10)
    [ $(( sounding * 10 )) -ge $(( periods * 9 )) ] || { echo "midi-test: too few periods with sound: $closed"; exit 1; }
    [ "$underruns" -eq 0 ] || { echo "midi-test: underruns: $closed"; exit 1; }
    expect "audio: closed after [0-9]* periods: [0-9]* client frames, 0 underruns" "the audio driver counted underruns"
    capture=$(grep -a -o "audio: capture checked: 1 ramp up, [0-9]* stream periods with [0-9]* client frames as filled, [0-9]* audible" "$log" | tail -1)
    [ -n "$capture" ] || { echo "midi-test: the capture was not checked"; exit 1; }
    stream=$(echo "$capture" | cut -d' ' -f7); audible=$(echo "$capture" | cut -d' ' -f16)
    [ $(( audible * 10 )) -ge $(( stream * 9 )) ] || { echo "midi-test: too few audible stream periods: $capture"; exit 1; }
    # The log server wrote every client's lines to the UART, each with its prefix.
    prefix='^\[ *[0-9]*\.[0-9]\{6\}\] I '
    for expected in "log-server: 3 clients, serial output attached" "midi-synth: ready at 44091710 mHz" "midi-player: music: summary of 256 periods" "audio: open for client 0"; do
      grep -a -q "${prefix}\(([0-9]* dropped) \)\?${expected}" "$serial" || { echo "midi-test: the UART got no line \"$expected\" from the log server"; exit 1; }
    done
    echo "{{ ok_label }} the player played D_E1M1 through the MIDI synthesiser and the audio driver: its first periods as on the host ($hash), $sounding of $periods periods with sound, $audible of $stream captured stream periods audible, 0 underruns; the log server wrote the lines to the UART"

# Build log-test's programs: the probe and the log server, for the image configuration `board` + `image_features`; only log-test bundles the probe, so it is not in `user_components`
[private]
build-log-probe board='rpi3' features='qemu' image_features=features: (_cross-build 'log-probe log-server' board user_link features '' image_features)

# Build the log-test e2e kernel: the PL011 driver, the log server and a probe whose Threads overflow a stack and panic
[group("test")]
build-log-test board='rpi3': (build-components board 'qemu' test_kernel_features) (build-log-probe board 'qemu' test_kernel_features) (_build-test-kernel 'log-test' board)

# Boot log-test: the probe's second Thread overflows its stack and its fault handler (liblog_client's, installed by Kickstart) logs `FAULT: log_probe thread 1: stack overflow (guard page)`; then its first Thread panics and the panic handler logs `PANICKED: log_probe: made to panic on purpose at …`, which ends the run. Both lines must reach the UART through the log server. The semihosting console goes to target/log-test.log, the UART to target/log-test-serial.log
[group("test")]
test-log board='rpi3' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (build-log-test board)
    #!/usr/bin/env bash
    set -uo pipefail
    log="{{ target_root }}/log-test.log"
    serial="{{ target_root }}/log-test-serial.log"
    rm -f "$log" "$serial"
    timeout 120 {{ qemu }} -M raspi3b -display none -monitor none \
      -chardev stdio,mux=on,id=char0,signal=off -serial "file:$serial" -semihosting-config enable=on,userspace=on,chardev=char0 \
      -dtb "{{ rpi3_dtb }}" -kernel "{{ images_dir / 'log-test-' + config + '.bin' }}" < /dev/null > "$log"
    status=$?
    grep -a "log-test: \|log-probe: \|FAULT: \|PANICKED" "$log" | tail -12
    cat "$serial"
    # The panic ends the run with a failure exit; a timeout (124) means it never came.
    [ "$status" -ne 124 ] || { echo "log-test: timed out; see $log"; exit 1; }
    grep -a -q "log-test: ✅ the probe has its fault handler" "$log" || { echo "log-test: the probe's fault handler was not installed; see $log"; exit 1; }
    grep -a -q "EL0 PANICKED: panicked at userspace/tests/log/probe/src/main.rs" "$log" || { echo "log-test: no panic on the semihosting console; see $log"; exit 1; }
    prefix='^\[ *[0-9]*\.[0-9]\{6\}\] '
    for expected in "I log-server: 1 clients, serial output attached" "I log-probe: ready" "E FAULT: log_probe thread 1: stack overflow (guard page) at pc 0x[0-9a-f]*, far 0x[0-9a-f]*, sp 0x[0-9a-f]*, esr 0x[0-9a-f]*" "E PANICKED: log_probe: made to panic on purpose at userspace/tests/log/probe/src/main.rs:[0-9]*"; do
      grep -a -q "${prefix}${expected}" "$serial" || { echo "log-test: the UART got no line \"$expected\" from the log server"; exit 1; }
    done
    echo "{{ ok_label }} the probe's stack overflow and panic reached the UART through the log server as FAULT: and PANICKED: lines"

# Build midi-test for the Raspberry Pi 3 hardware (no QEMU, no semihosting): the player plays Doom's D_E1M1 through the MIDI synthesiser and the PWM audio driver on the headphone jack, looping, for good, and the log server writes the player's, the synthesiser's and the audio driver's lines on the UART
[group("hw")]
build-midi-test-hw board='rpi3': (build-components board '' '') (build-midi-player board '' '') (_build-test-kernel 'midi-test' board '')

# Build midi-test for the Raspberry Pi 3 and boot it via chainofcommand (e.g. `just chainboot_serial=/dev/ttyUSB0 chainboot_load_baud=1500000 boot-midi-test`): with headphones in the jack and `dtoverlay=pwm-2chan,pin=40,func=4,pin2=41,func2=4` in config.txt, the player plays Doom's E1M1 music, looping, at volume 64, and nothing else runs. The UART shows the song's timing and first notes, the ARM and core clocks, and every 256 periods (about 3 s) the music summary (`service` min/avg/max against the period, packets, periods with sound, playing, the longest refill, underruns), the level (peak and RMS), the synthesiser's peak voice count and the song position
[group("hw")]
boot-midi-test: build-midi-test-hw chainofcommand
    target/debug/chainofcommand {{ chainofcommand_args }} --kernel "{{ images_dir / 'midi-test-rpi3.bin' }}"

# Build the window-events-test e2e kernel: compositor-test's components plus window-events, a client checking the events of its two windows
[group("test")]
build-window-events-test board='rpi3': (build-components board 'qemu' test_kernel_features) (_build-test-kernel 'window-events-test' board)

# Boot window-events-test on raspi3b with a USB keyboard and mouse behind a hub: once window-events has made its windows (B, the newest, has the focus), click on A, drag over B, release and type `a` through the QEMU monitor; window-events checks what each window received and ends the run with the result. The console goes to target/window-events-test.log
[group("test")]
test-window-events board='rpi3' seconds='120' features=test_kernel_features config=(board + '-' + replace(features, ',', '+') + profile_suffix): (build-window-events-test board)
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
    monitor_command() { printf '%s\n' "$1" | nc -N -U "$monitor" > /dev/null; }
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
[private]
build-chainboot board='rpi4' features='' config=(board + (if features == '' { '' } else { '-' + replace(features, ',', '+') }) + profile_suffix): (_cross-build 'chainboot' board chainboot_link features)
    @mkdir -p "{{ images_dir }}"
    {{ objcopy }} --strip-all -O binary "{{ target_root / config / target / image_profile / 'chainboot' }}" "{{ images_dir / 'chainboot-' + config + '.bin' }}"
    @just _audit-chainboot-exclusives "{{ target_root / config / target / image_profile / 'chainboot' }}"
    @echo "{{ok_label}} chainboot built for {{ board }}{{ if features != '' { ' [' + features + ']' } else { '' } }}: {{ images_dir / 'chainboot-' + config + '.bin' }}"

# Fail if the chainboot ELF holds an exclusive or atomic read-modify-write instruction: chainboot runs without the data cache, where the Pi's cores never complete them and the program hangs without a sound. QEMU completes them, so only this check (and the board) can tell.
[private]
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

# Build wood-test and run it in QEMU with a display window and the audio test tree: the compositor's screen with the lava lamp and Wood playing Doom's demos, its sound effects going to the audio driver's capture buffer (QEMU has no PWM to hear them on). Wood's window has the keyboard focus as the newest window; clicking a window (the USB mouse behind the hub; click into QEMU's window to grab the pointer, Ctrl-Alt-G releases it) moves the focus. The console (serial and semihosting) goes to the terminal and qemu.log
[group("emu")]
qemu-wood features='qemu': (build-components 'rpi3' 'qemu' features) (build-wood 'rpi3' 'qemu' features) (_build-test-kernel 'wood-test' 'rpi3' features) _audio-test-dtb
    @rm -f qemu.log
    {{ qemu }} {{ qemu_base_opts }} {{ qemu_usb }} -device usb-mouse,port=1.2 -dtb "{{ audio_test_dtb }}" -kernel "{{ images_dir / 'wood-test-rpi3-' + replace(features, ',', '+') + profile_suffix + '.bin' }}"

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
cb-eject board='rpi4': (build-chainboot board)
    cp "{{ images_dir / 'chainboot-' + board + '.bin' }}" "{{ volume }}/{{ 'chainboot-' + board + '.bin' }}"
    @echo "{{copy_label}} copied chainboot to {{ volume }}/{{ 'chainboot-' + board + '.bin' }}"
    # TODO: sed proper kernel name here in config.txt
    diskutil unmountDisk "{{ volume }}"

# Build and boot via chainofcommand
[group("hw")]
boot: build chainofcommand
    target/debug/chainofcommand {{ chainofcommand_args }} --kernel "{{ kernel_rpi4_bin }}"

# Build and boot in qemu via chainofcommand
[group("emu")]
boot-qemu: (build 'rpi3' 'qemu') chainofcommand
    target/debug/chainofcommand {{ chainofcommand_args }} --kernel "{{ kernel_rpi3_qemu_bin }}"

# === Openocd ===

# Start openocd connected via JTAG
[group("hw")]
openocd board='rpi4':
    {{ openocd_bin }} -f interface/jlink.cfg -f ../ocd/{{ board }}_target.cfg

alias ocd := openocd

# === Testing ===

# Run device and chainboot tests in QEMU (rpi3), plus capability and tool tests natively
[group("test")]
test: test-device test-chainboot test-host test-debug-console test-key-table test-untyped test-capability test-memory test-sync test-ppc test-preempt test-preempt-rpi4 test-dispatch test-dispatch-rpi4 test-irq test-irq-rpi4 test-driver test-driver-rpi4 test-mailbox test-mailbox-rpi4 test-uart test-uart-rpi4 test-endpoint test-fp-trap test-fault test-usb test-usb-core test-usb-hid test-compositor test-sd test-audio test-dma

alias t := test

# Run device crate tests in QEMU (rpi3) --verbose
[group("test")]
test-device: (_cross-build 'nucleus' 'rpi3' nucleus_link 'qemu') (build-privileged 'rpi3' 'qemu') (build-drivers 'rpi3' 'qemu')
    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ device_test_rustflags }}" \
    cargo test --tests {{ target_json }} --features=qemu {{ rust_std }} \
      --workspace --exclude=chainofcommand --exclude=vesper-image-build --exclude=metta-meddle --exclude=meddler --exclude=chainboot --exclude=sf2-trim --exclude=wood-synth

    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ device_test_rustflags }}" \
    cargo test --doc {{ target_json }} --features=qemu {{ rust_std }} \
    --workspace --exclude=chainofcommand --exclude=vesper-image-build --exclude=metta-meddle --exclude=meddler --exclude=chainboot --exclude=sf2-trim --exclude=wood-synth

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

# Boot driver-test: Kickstart loads a probe driver per matching device tree node and hands it registers, interrupts, memory and a Time; the PL011 probe's high-priority Thread must wake on time beside two spinning normal Threads (the root dispatcher's priorities)
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
test-host: test-object-host test-platform-host test-meddle test-usb-host test-input-host test-compositor-host test-dispatcher-host test-wood-synth (_host-test 'vesper-chainboot-protocol' 'chainboot_protocol')
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
test-platform-host: (_host-test 'vesper-devicetree' 'devicetree') (_host-test 'vesper-image' 'privileged') (_host-test 'irqchip-bcm2836' 'bcm2836') (_host-test 'irqchip-gicv2' 'gicv2') (_host-test 'vesper-io-channel' 'io_channel') (_host-test 'vesper-serial-channel' 'serial_channel') (_host-test 'vesper-block-channel' 'block_channel') (_host-test 'uart-pl011' 'pl011') (_host-test 'sdhost-bcm2835' 'sdhost') (_host-test 'gpio-bcm2835' 'gpio')(_host-test 'vesper-device-driver' 'areas') (_host-test 'vesper-device-driver' 'masters') (_host-test 'mailbox-impl' 'tags') (_host-test 'power-impl' 'pm') (_host-test 'cprman-bcm2835' 'cprman') (_host-test 'dma-bcm2835' 'dma') (_host-test 'audio-pwm-bcm2835' 'pwm') (_host-test 'audio-pwm-bcm2835' 'stream') (_host-test 'fb-impl' 'requests') (_host-test 'shell' 'commands') (_host-test 'vesper-graphics' 'graphics')

# Run the Meddle interface compiler tests and the generated code's runtime tests natively
[group("test")]
test-meddle: (_host-test 'metta-meddle-runtime' 'words')
    cargo test -p metta-meddle

# Run the USB protocol logic tests natively: setup packets, descriptors, enumeration, HID boot reports
[group("test")]
test-usb-host: (_host-test 'vesper-usb-protocol' 'protocol') (_host-test 'usb-hcd-dwc2' 'dwc2') (_host-test 'usb-core' 'usb_core') (_host-test 'usb-hid' 'hid')

# Run the input event library tests natively: record layout, descriptor, state tracker
[group("test")]
test-input-host: (_host-test 'vesper-input-event' 'input_event')

# Run the compositor logic tests natively: windows, hit-testing, focus, grab and input routing
[group("test")]
test-compositor-host: (_host-test 'metta-compositor-core' 'compositor_core')

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
      --workspace --exclude=chainofcommand --exclude=vesper-image-build --exclude=metta-meddle --exclude=meddler --exclude=sf2-trim \
      -- --deny warnings --allow deprecated

# Run embedded clippy checks (all feature combos) and capability host-test linting
[group("maintenance")]
clippy: (build 'rpi3' 'qemu') (build-components 'rpi3' 'qemu') (build-wood 'rpi3' 'qemu') (_cross-clippy 'rpi3' '') (_cross-clippy 'rpi4' '') (_cross-clippy 'rpi3' 'noserial') (_cross-clippy 'rpi3' 'qemu') (_cross-clippy 'rpi3' 'noserial,qemu') (_cross-clippy 'rpi3' 'jtag') (_cross-clippy 'rpi3' 'noserial,jtag') (build 'rpi3' 'qemu,debug_kernel') (_cross-clippy 'rpi3' 'debug_kernel') (_cross-clippy 'rpi3' 'qemu,debug_kernel') (_cross-clippy 'rpi3' 'qemu,fp_trap_test') (_cross-clippy 'rpi3' 'mmu') clippy-object-host clippy-platform-host clippy-meddle clippy-usb-host clippy-input-host clippy-compositor-host

# Run shortened clippy (default features on both boards) and capability host-test linting
[group("maintenance")]
clippy-pre-push: (build 'rpi3' 'qemu') (_cross-clippy 'rpi3' '') (_cross-clippy 'rpi4' '') clippy-object-host clippy-platform-host clippy-meddle clippy-usb-host clippy-input-host clippy-compositor-host

# Lint one opt-in (`host-tests`) integration test with its native host harness
[private]
_host-clippy package test features='host-tests':
    RUSTFLAGS="{{ fixed_rustflags }}" \
    cargo clippy -p {{ package }} --features={{ features }} --test {{ test }} \
      -- --deny warnings --allow deprecated

# Lint the opt-in capability ABI tests with their native host harness
[private]
clippy-object-host: (_host-clippy 'vesper-objects' 'object_type') (_host-clippy 'vesper-objects' 'object_type' 'host-tests,debug_kernel')

# Lint the boot-platform host tests
[private]
clippy-platform-host: (_host-clippy 'vesper-devicetree' 'devicetree') (_host-clippy 'vesper-image' 'privileged') (_host-clippy 'irqchip-bcm2836' 'bcm2836') (_host-clippy 'irqchip-gicv2' 'gicv2') (_host-clippy 'vesper-io-channel' 'io_channel') (_host-clippy 'vesper-serial-channel' 'serial_channel') (_host-clippy 'vesper-block-channel' 'block_channel') (_host-clippy 'uart-pl011' 'pl011') (_host-clippy 'sdhost-bcm2835' 'sdhost') (_host-clippy 'gpio-bcm2835' 'gpio')(_host-clippy 'vesper-device-driver' 'areas') (_host-clippy 'vesper-device-driver' 'masters') (_host-clippy 'mailbox-impl' 'tags') (_host-clippy 'power-impl' 'pm') (_host-clippy 'cprman-bcm2835' 'cprman') (_host-clippy 'dma-bcm2835' 'dma') (_host-clippy 'audio-pwm-bcm2835' 'pwm') (_host-clippy 'audio-pwm-bcm2835' 'stream') (_host-clippy 'fb-impl' 'requests') (_host-clippy 'shell' 'commands') (_host-clippy 'vesper-graphics' 'graphics')

# Lint the Meddle interface compiler (library, meddler and tests) and the runtime host tests
[private]
clippy-meddle: (_host-clippy 'metta-meddle-runtime' 'words')
    cargo clippy -p metta-meddle -p meddler --tests -- --deny warnings --allow deprecated

# Cross-clippy only the USB stack crates (controller, core, host library, HID) for a board with a feature set
[private]
_cross-clippy-usb board='rpi3' features='':
    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ fixed_rustflags }} {{ if board == 'rpi3' { board_rpi3_flags } else { board_rpi4_flags } }}" \
    cargo clippy {{ target_json }} \
      {{ if features != '' { '--features=' + features } else { '' } }} \
      {{ rust_std }} \
      -p usb-hcd-dwc2 -p usb-core -p vesper-usb-host -p vesper-usb-protocol -p usb-hid -p input-trace \
      -- --deny warnings --allow deprecated

# Lint just the USB stack: the target build with and without `qemu`, and the host tests (much shorter than `clippy`)
[group("maintenance")]
clippy-usb: (_cross-clippy-usb 'rpi3' '') (_cross-clippy-usb 'rpi3' 'qemu') clippy-usb-host

# Cross-clippy only Wood (the Room engine's Vesper program) for a board with a feature set
[private]
_cross-clippy-wood board='rpi3' features='':
    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ fixed_rustflags }} {{ if board == 'rpi3' { board_rpi3_flags } else { board_rpi4_flags } }}" \
    cargo clippy {{ target_json }} \
      {{ if features != '' { '--features=' + features } else { '' } }} \
      {{ rust_std }} \
      -p wood \
      -- --deny warnings --allow deprecated

# Lint just Wood: the target build with and without `qemu` (much shorter than `clippy`)
[group("maintenance")]
clippy-wood: (_cross-clippy-wood 'rpi3' '') (_cross-clippy-wood 'rpi3' 'qemu')

# Cross-clippy only the audio crates (the clock manager, DMA and PWM audio components, their interface crates and the audio test client) for a board with a feature set
[private]
_cross-clippy-audio board='rpi3' features='':
    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ fixed_rustflags }} {{ if board == 'rpi3' { board_rpi3_flags } else { board_rpi4_flags } }}" \
    cargo clippy {{ target_json }} \
      {{ if features != '' { '--features=' + features } else { '' } }} \
      {{ rust_std }} \
      -p cprman-bcm2835 -p clock -p clock-client -p dma-bcm2835 -p dma -p dma-client -p audio-pwm-bcm2835 -p audio -p audio-client -p audio-test-client \
      -- --deny warnings --allow deprecated

# Lint just the audio crates: the target build with and without `qemu`, and their host tests (much shorter than `clippy`)
[group("maintenance")]
clippy-audio: (_cross-clippy-audio 'rpi3' '') (_cross-clippy-audio 'rpi3' 'qemu') (_host-clippy 'cprman-bcm2835' 'cprman') (_host-clippy 'dma-bcm2835' 'dma') (_host-clippy 'audio-pwm-bcm2835' 'pwm') (_host-clippy 'audio-pwm-bcm2835' 'stream')

# Cross-clippy only the log crates (the log server component, its log_v1 interface crates, the client library, libserial-trace, which sends through it, and log-test's probe) for a board with a feature set
[private]
_cross-clippy-log board='rpi3' features='':
    VESPER_ARTIFACT_DIR="{{ rpi3_qemu_artifacts }}" RUSTFLAGS="{{ fixed_rustflags }} {{ if board == 'rpi3' { board_rpi3_flags } else { board_rpi4_flags } }}" \
    cargo clippy {{ target_json }} \
      {{ if features != '' { '--features=' + features } else { '' } }} \
      {{ rust_std }} \
      -p log-server -p log-interface -p log-interface-client -p vesper-log-client -p vesper-serial-trace -p log-probe \
      -- --deny warnings --allow deprecated --allow clippy::map_unwrap_or --allow clippy::doc_markdown

# Lint just the log crates: the target build with and without `qemu`, and their host tests (much shorter than `clippy`)
[group("maintenance")]
clippy-log: (_cross-clippy-log 'rpi3' '') (_cross-clippy-log 'rpi3' 'qemu') (_host-clippy 'log-server' 'protocol')

# Run the log server's host tests: the line header, the client's buffer set, line cutting and the server's output line
[group("test")]
test-log-host: (_host-test 'log-server' 'protocol')

# Run the root dispatcher's host tests: the highest runnable priority first, round-robin within a priority, no starvation guard
[group("test")]
test-dispatcher-host: (_host-test 'root-dispatcher' 'policy')

# Lint Wood's music synthesiser: the library, the MIDI synthesiser component and midi-test's player (with and without `qemu`) for the target, their host tests, and the bank-trimming and song-export tool
[group("maintenance")]
clippy-wood-synth: (_host-clippy 'wood-synth' 'synth') (_host-clippy 'wood-synth' 'doom_music') (_host-clippy 'midi-synth' 'session') (_host-clippy 'midi-player' 'song')
    RUSTFLAGS="{{ fixed_rustflags }} {{ board_rpi3_flags }}" \
    cargo clippy {{ target_json }} {{ rust_std }} -p wood-synth -p midi-synth -p midi-synth-server -p midi-synth-client -p midi-player \
      -- --deny warnings --allow deprecated
    RUSTFLAGS="{{ fixed_rustflags }} {{ board_rpi3_flags }}" \
    cargo clippy {{ target_json }} {{ rust_std }} -p midi-synth -p midi-player --features qemu \
      -- --deny warnings --allow deprecated
    RUSTFLAGS="{{ fixed_rustflags }}" cargo clippy -p sf2-trim --all-targets -- --deny warnings --allow deprecated

# Check that Wood's music synthesiser calls no soft-float routine in its target build (audio decision D9: integer only): the wood-synth library, and the linked MIDI synthesiser component, which must hold none of them; the core library is checked too, to show the pattern finds them
[group("maintenance")]
synth-no-float: (_cross-build 'midi-synth' 'rpi3' user_link '')
    #!/usr/bin/env bash
    set -euo pipefail
    # A dev build of its own: its core keeps the float code that shows the
    # pattern works (a release core with LTO references none).
    export CARGO_TARGET_DIR="{{ target_root / 'synth-no-float' }}"
    RUSTFLAGS="{{ fixed_rustflags }} {{ board_rpi3_flags }}" \
      cargo build {{ target_json }} {{ rust_std }} -p wood-synth
    artifacts="$CARGO_TARGET_DIR/{{ target }}/debug"
    pattern='^__(add|sub|mul|div|neg|pow)[sdt]f[23]$|^__(fix|fixuns)[sdt]f[sdt]i$|^__float(un)?[sdt]i[sdt]f$|^__(extend|trunc)[sdt]f[sdt]f2$|^__(eq|ne|lt|le|gt|ge|un|cmp)[sdt]f2$|^__powi[sdt]f2$'
    calls() { rust-nm --undefined-only "$1" 2>/dev/null | awk '{print $2}' | grep -E "$pattern" | sort -u || true; }
    core=$(ls "$artifacts"/build/core/*/out/libcore-*.rlib | head -1)
    control=$(calls "$core" | wc -l)
    echo "core (control): $control soft-float routines referenced"
    if [ "$control" -eq 0 ]; then echo "the control found none: the check is not working"; exit 1; fi
    found=$(calls "$artifacts/libwood_synth.rlib")
    if [ -n "$found" ]; then echo "wood-synth calls soft-float routines:"; echo "$found"; exit 1; fi
    echo "wood-synth: no soft-float calls"
    # The linked component: LTO keeps a soft-float routine only if something calls it.
    component="{{ target_root / 'rpi3' + profile_suffix / target / image_profile / 'midi-synth' }}"
    linked=$(rust-nm "$component" | awk '{print $NF}' | grep -E "$pattern" | sort -u || true)
    if [ -n "$linked" ]; then echo "the midi-synth component links soft-float routines:"; echo "$linked"; exit 1; fi
    echo "midi-synth component: no soft-float routines linked"

# Run the music synthesiser's host tests: tables, zones, envelope, pitch, loops, controllers, sequencer timing, render-split independence, no floating point, and on the shareware IWAD's music with the trimmed SC-55 bank (what it keeps, rustysynth loading it, golden hashes, loudness against rustysynth); and the MIDI synthesiser component's midi_synth_v1 session (packets, chunked songs, controls, refusals, the start hashes test-wood and test-midi check, the peak voice count); midi-test's player library (the song's timing and first notes, the position, the level meter); and the song export of sf2-trim
[group("test")]
test-wood-synth: (_host-test 'wood-synth' 'synth') (_host-test 'wood-synth' 'doom_music') (_host-test 'midi-synth' 'session') (_host-test 'midi-player' 'song')
    cargo test -p sf2-trim

# Time the synthesiser on the host in release mode: every shareware song for 60 s in 512-frame periods, ns per frame and per voice-frame (doc/agentic/audio-profiling.md)
[group("test")]
profile-synth:
    RUSTFLAGS="{{ fixed_rustflags }}" \
    cargo test --release -p wood-synth --features=host-tests --test doom_music -- --ignored --nocapture --test-threads=1 profile_render

# Trim the SC-55 SoundFont to the programs and drum notes of the shareware IWAD's music (audio decision D9.4), into target/wood/sc55-doom1.sf2
[group("tools")]
wood-soundfont:
    cargo run -p sf2-trim -- --wad userspace/wood/wad/doom1.wad \
      --soundfont "userspace/wood/room/soundfonts/SC55Soundfont-1.2b/SC-55 SoundFont v1.2b.sf2" \
      --output "{{ wood_soundfont }}"

# Export Doom's E1M1 music from the shareware IWAD as a Standard MIDI File, converted by Room's mus2midi as the game does, into target/wood/d_e1m1.mid (the song midi-test plays)
[group("tools")]
wood-midi:
    cargo run -p sf2-trim -- --wad userspace/wood/wad/doom1.wad --song D_E1M1 --midi "{{ wood_midi }}"

# Lint the USB protocol host tests with their native host harness
[private]
clippy-usb-host: (_host-clippy 'vesper-usb-protocol' 'protocol') (_host-clippy 'usb-hcd-dwc2' 'dwc2') (_host-clippy 'usb-core' 'usb_core') (_host-clippy 'usb-hid' 'hid')

# Lint the input event library tests with their native host harness
[private]
clippy-input-host: (_host-clippy 'vesper-input-event' 'input_event')

# Lint the compositor logic tests with their native host harness
[private]
clippy-compositor-host: (_host-clippy 'metta-compositor-core' 'compositor_core')

# Clippy for the host tools (chainofcommand and its protocol crate's host tests, vesper-image-build)
[private]
_clippy-coc: (_host-clippy 'vesper-chainboot-protocol' 'chainboot_protocol')
    cargo clippy -p vesper-image-build -- --deny warnings --allow deprecated # FIXME Shouldn't be here!
    cargo clippy -p chainofcommand --all-targets -- --deny warnings --allow deprecated

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
[private]
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
