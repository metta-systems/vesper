# The wood-test hardware image

What `just build-wood-test-hw` bundles (`kernel/tests/wood-test/image.toml`) and
how the parts are wired. An arrow goes from the importer to the exporter and is
labelled with the interface (`imports.if`). An interface marked *channel* also
carries an io-channel: Kickstart gives the importer a control page, data pages and
an EventCount. Kickstart loads everything and makes the connections at boot. The
root dispatcher then runs every program's Threads.

The hardware image is built without `qemu`, so:

- the audio driver streams through a circular DMA ring;
- Wood's log is dropped;
- the compositor traces routed input on the UART.

The audio driver loads only when `config.txt` has
`dtoverlay=pwm-2chan,pin=40,func=4,pin2=41,func2=4`. Without it Wood runs silent.

```mermaid
flowchart LR
    subgraph hardware["BCM2837 (Raspberry Pi 3 B)"]
        hw_usb["DWC2 USB OTG"]
        hw_mbox["VideoCore mailbox"]
        hw_fb["framebuffer"]
        hw_uart["PL011 UART"]
        hw_gpio["GPIO 40/41"]
        hw_cprman["CPRMAN (PWM clock)"]
        hw_dma["DMA controller"]
        hw_pwm["PWM → headphone jack"]
    end

    subgraph drivers["Drivers (bound to device-tree nodes)"]
        usb["usb<br/>usb-hcd-dwc2"]
        mailbox["mailbox"]
        fb["fb"]
        uart["uart<br/>uart-pl011"]
        gpio["gpio<br/>gpio-bcm2835"]
        cprman["cprman<br/>cprman-bcm2835"]
        dma["dma<br/>dma-bcm2835"]
        audio["audio<br/>audio-pwm-bcm2835"]
    end

    subgraph programs["Components"]
        usb_core["usb_core<br/>usb-core"]
        usb_hid["usb_hid<br/>usb-hid"]
        compositor["compositor"]
        draw_demo["draw_demo<br/>(lava lamp window)"]
        wood["wood<br/>Room Doom engine"]
        midi_synth["midi_synth<br/>midi-synth"]
        dispatcher["dispatcher<br/>root-dispatcher"]
    end

    subgraph resources["Resources"]
        font[("font<br/>font8x8.bin")]
        wad[("doom1_wad<br/>doom1.wad")]
        soundfont[("soundfont<br/>sc55-doom1.sf2")]
    end

    %% Input: keyboard and mouse over USB
    usb_core -->|"usb_hcd_v1 (channel)"| usb
    usb_hid -->|"usb_v1"| usb_core
    usb_hid -->|"usb_hcd_v1 (channel)"| usb
    usb -->|"mailbox_v1: set_power_state"| mailbox

    %% Display
    compositor -->|"input_v1 (channel)"| usb_hid
    compositor -->|"fb_v1"| fb
    compositor -->|"serial channel"| uart
    fb -->|"mailbox_v1: property"| mailbox
    draw_demo -->|"compositor_v1"| compositor
    wood -->|"compositor_v1 (channel: window events)"| compositor

    %% Sound
    wood -->|"midi_synth_v1 (channel: MIDI in, frames out)"| midi_synth
    wood -->|"audio_v1 (channel: s16 stereo)"| audio
    audio -->|"clock_v1"| cprman
    audio -->|"gpio_v1: set_function"| gpio
    audio -->|"dma_v1 (channel: ring)"| dma

    %% Bundled data
    draw_demo -.-> font
    wood -.-> wad
    midi_synth -.-> soundfont

    %% Devices each driver owns
    usb --- hw_usb
    mailbox --- hw_mbox
    fb --- hw_fb
    uart --- hw_uart
    gpio --- hw_gpio
    cprman --- hw_cprman
    dma --- hw_dma
    audio --- hw_pwm
```

## Notes

- **Wood's threads.** The engine Thread draws frames and reads keys from the
  compositor. Its `music_*` and `sound_*` methods only queue work. Each period,
  Wood's playback Thread:
  1. sends the queued MIDI commands to `midi_synth`;
  2. pulls one rendered period, which `midi_synth` renders inside the `service`
     call;
  3. mixes the sound effects on top;
  4. queues the result on `audio_v1`.
- **The audio path.**
  - The audio driver converts the s16 frames to PWM words in its DMA ring.
  - `dma` alone holds the DMA controller's page.
  - `cprman` sets PWM clock 30 to 100 MHz (PLLD/5).
- **Input.** `usb_hid` reads reports over interrupt pipes. The compositor
  routes them to the focused window, Wood's or the lava lamp's.
- **The dispatcher** is bound last. It schedules the Threads of every component
  above.
