//! Host tests of the window list and the input router.

#[cfg(test)]
mod tests {
    use {
        compositor_core::{
            router::{Output, Routed, Router},
            windows::{MAX_WINDOWS, Rect, Window, Windows},
        },
        vesper_input_event::{
            record::{Event, KEY_DOWN, KEY_UP, Kind},
            usage::{code, generic_desktop, page},
        },
    };

    const LEFT: u32 = 0x0009_0001;
    const RIGHT: u32 = 0x0009_0002;
    const A: u32 = 0x0007_0004;

    fn window(id: u32, x: i32, y: i32, width: u32, height: u32) -> Window {
        Window {
            id,
            client: 1,
            rect: Rect {
                x,
                y,
                width,
                height,
            },
        }
    }

    /// 100x100 windows 1 and 2, overlapping at 50..100, on a 400x300 screen with
    /// the pointer at 200,150.
    fn router() -> Router {
        let mut router = Router::new(400, 300);
        assert!(router.windows_mut().add(window(1, 0, 0, 100, 100)));
        assert!(router.windows_mut().add(window(2, 50, 50, 100, 100)));
        router
    }

    fn key(code: u32, value: i32) -> Event {
        Event::new(5, 1, Kind::Key, code, value)
    }

    fn rel(usage: u16, value: i32) -> Event {
        Event::new(5, 1, Kind::Rel, code(page::GENERIC_DESKTOP, usage), value)
    }

    fn sync() -> Event {
        Event::sync(5, 1)
    }

    /// Feed `events` and a closing `Sync`; what the windows receive.
    fn frame(router: &mut Router, events: &[Event]) -> Vec<Routed> {
        let mut out = Output::new();
        for event in events {
            router.feed(event, &mut out);
        }
        router.feed(&sync(), &mut out);
        out.items().to_vec()
    }

    fn kinds(routed: &[Routed], window: u32) -> Vec<(Kind, u32, i32)> {
        routed
            .iter()
            .filter(|r| r.window == window)
            .map(|r| (r.event.kind, r.event.code, r.event.value))
            .collect()
    }

    fn x(value: i32) -> (Kind, u32, i32) {
        (
            Kind::Abs,
            code(page::GENERIC_DESKTOP, generic_desktop::X),
            value,
        )
    }

    fn y(value: i32) -> (Kind, u32, i32) {
        (
            Kind::Abs,
            code(page::GENERIC_DESKTOP, generic_desktop::Y),
            value,
        )
    }

    fn move_to(router: &mut Router, target_x: i32, target_y: i32) -> Vec<Routed> {
        let (px, py) = router.pointer();
        frame(
            router,
            &[
                rel(generic_desktop::X, target_x - px),
                rel(generic_desktop::Y, target_y - py),
            ],
        )
    }

    #[test]
    fn windows_stack_hit_test_raise_and_go_away() {
        let mut windows = Windows::new();
        assert!(windows.add(window(1, 0, 0, 100, 100)));
        assert!(windows.add(window(2, 50, 50, 100, 100)));
        assert!(!windows.add(window(2, 0, 0, 1, 1)), "ids are unique");
        assert_eq!(windows.top_at(75, 75), Some(2), "the later one is on top");
        assert_eq!(windows.top_at(10, 10), Some(1));
        assert_eq!(windows.top_at(149, 149), Some(2));
        assert_eq!(windows.top_at(150, 150), None, "the far edge is outside");
        assert_eq!(windows.top_at(-1, 0), None);
        assert!(windows.raise(1));
        assert_eq!(windows.top_at(75, 75), Some(1));
        assert!(windows.move_to(1, 200, 200));
        assert_eq!(windows.top_at(75, 75), Some(2));
        assert_eq!(windows.top_at(250, 250), Some(1));
        assert!(windows.remove(1));
        assert!(!windows.remove(1));
        assert_eq!(windows.len(), 1);
        let order: Vec<u32> = windows.bottom_up().map(|w| w.id).collect();
        assert_eq!(order, [2]);
    }

    #[test]
    fn the_window_table_has_a_limit() {
        let mut windows = Windows::new();
        for id in 0..u32::try_from(MAX_WINDOWS).unwrap() {
            assert!(windows.add(window(id, 0, 0, 1, 1)));
        }
        assert!(!windows.add(window(99, 0, 0, 1, 1)));
    }

    #[test]
    fn the_pointer_moves_by_deltas_and_stays_on_the_screen() {
        let mut router = router();
        assert_eq!(router.pointer(), (200, 150));
        frame(
            &mut router,
            &[rel(generic_desktop::X, 30), rel(generic_desktop::Y, -20)],
        );
        assert_eq!(router.pointer(), (230, 130));
        frame(
            &mut router,
            &[
                rel(generic_desktop::X, 5000),
                rel(generic_desktop::Y, -5000),
            ],
        );
        assert_eq!(router.pointer(), (399, 0));
        // Motion of one frame adds up.
        frame(
            &mut router,
            &[rel(generic_desktop::X, -10), rel(generic_desktop::X, -10)],
        );
        assert_eq!(router.pointer(), (379, 0));
    }

    #[test]
    fn entering_a_window_sends_enter_and_the_local_position() {
        let mut router = router();
        let sent = move_to(&mut router, 20, 30);
        assert_eq!(router.hover(), Some(1));
        assert_eq!(
            kinds(&sent, 1),
            [(Kind::Enter, 0, 0), x(20), y(30), (Kind::Sync, 0, 0)]
        );
        assert!(sent.iter().all(|r| r.event.subject == r.window));
        // Moving inside sends the position again.
        let sent = move_to(&mut router, 25, 35);
        assert_eq!(kinds(&sent, 1), [x(25), y(35), (Kind::Sync, 0, 0)]);
        // Crossing into the window on top leaves one and enters the other, in
        // the entered window's coordinates.
        let sent = move_to(&mut router, 60, 60);
        assert_eq!(router.hover(), Some(2));
        assert_eq!(kinds(&sent, 1), [(Kind::Leave, 0, 0), (Kind::Sync, 0, 0)]);
        assert_eq!(
            kinds(&sent, 2),
            [(Kind::Enter, 0, 0), x(10), y(10), (Kind::Sync, 0, 0)]
        );
        // Leaving every window.
        let sent = move_to(&mut router, 300, 250);
        assert_eq!(router.hover(), None);
        assert_eq!(kinds(&sent, 2), [(Kind::Leave, 0, 0), (Kind::Sync, 0, 0)]);
    }

    #[test]
    fn a_press_focuses_raises_and_grabs() {
        let mut router = router();
        move_to(&mut router, 20, 20);
        let sent = frame(&mut router, &[key(LEFT, KEY_DOWN)]);
        assert_eq!(router.focus(), Some(1));
        assert_eq!(
            kinds(&sent, 1),
            [
                (Kind::Focus, 0, 1),
                (Kind::Key, LEFT, KEY_DOWN),
                (Kind::Sync, 0, 0)
            ]
        );
        // The window pressed on is on top now: at 75,75 it wins over 2.
        assert_eq!(router.windows().top_at(75, 75), Some(1));
    }

    #[test]
    fn the_grab_keeps_pointer_events_with_the_window_pressed_on() {
        let mut router = router();
        move_to(&mut router, 20, 20);
        frame(&mut router, &[key(LEFT, KEY_DOWN)]);
        // Drag across window 2 and out of the screen's windows entirely.
        let sent = move_to(&mut router, 120, 120);
        assert_eq!(router.hover(), Some(1), "the grab holds");
        assert_eq!(kinds(&sent, 1), [x(120), y(120), (Kind::Sync, 0, 0)]);
        assert!(kinds(&sent, 2).is_empty());
        // A second button is the grab's too.
        let sent = frame(&mut router, &[key(RIGHT, KEY_DOWN)]);
        assert_eq!(
            kinds(&sent, 1),
            [(Kind::Key, RIGHT, KEY_DOWN), (Kind::Sync, 0, 0)]
        );
        // Releasing one button keeps the grab; releasing the last ends it and
        // the pointer's events go to what is under it.
        frame(&mut router, &[key(LEFT, KEY_UP)]);
        assert_eq!(router.hover(), Some(1));
        let sent = frame(&mut router, &[key(RIGHT, KEY_UP)]);
        assert_eq!(
            kinds(&sent, 1),
            [
                (Kind::Key, RIGHT, KEY_UP),
                (Kind::Leave, 0, 0),
                (Kind::Sync, 0, 0)
            ]
        );
        assert_eq!(router.hover(), Some(2), "window 2 is under 120,120");
        assert_eq!(
            kinds(&sent, 2),
            [(Kind::Enter, 0, 0), x(70), y(70), (Kind::Sync, 0, 0)]
        );
    }

    #[test]
    fn a_press_on_the_background_takes_the_focus_away() {
        let mut router = router();
        move_to(&mut router, 20, 20);
        frame(&mut router, &[key(LEFT, KEY_DOWN)]);
        frame(&mut router, &[key(LEFT, KEY_UP)]);
        move_to(&mut router, 300, 250);
        let sent = frame(&mut router, &[key(LEFT, KEY_DOWN)]);
        assert_eq!(router.focus(), None);
        assert_eq!(kinds(&sent, 1), [(Kind::Focus, 0, 0), (Kind::Sync, 0, 0)]);
        frame(&mut router, &[key(LEFT, KEY_UP)]);
    }

    #[test]
    fn focus_moves_from_one_window_to_another() {
        let mut router = router();
        move_to(&mut router, 20, 20);
        frame(&mut router, &[key(LEFT, KEY_DOWN)]);
        frame(&mut router, &[key(LEFT, KEY_UP)]);
        move_to(&mut router, 120, 120);
        let sent = frame(&mut router, &[key(LEFT, KEY_DOWN)]);
        assert_eq!(router.focus(), Some(2));
        assert_eq!(kinds(&sent, 1), [(Kind::Focus, 0, 0), (Kind::Sync, 0, 0)]);
        assert_eq!(
            kinds(&sent, 2),
            [
                (Kind::Focus, 0, 1),
                (Kind::Key, LEFT, KEY_DOWN),
                (Kind::Sync, 0, 0)
            ]
        );
    }

    #[test]
    fn keys_go_to_the_focused_window_and_are_counted_when_there_is_none() {
        let mut router = router();
        let sent = frame(&mut router, &[key(A, KEY_DOWN)]);
        assert!(sent.is_empty());
        assert_eq!(router.counters().unrouted, 1);
        move_to(&mut router, 20, 20);
        frame(&mut router, &[key(LEFT, KEY_DOWN)]);
        frame(&mut router, &[key(LEFT, KEY_UP)]);
        // The pointer is elsewhere: keys still go to the focus.
        move_to(&mut router, 300, 250);
        let sent = frame(&mut router, &[key(A, KEY_DOWN), key(A, KEY_UP)]);
        assert_eq!(
            kinds(&sent, 1),
            [
                (Kind::Key, A, KEY_DOWN),
                (Kind::Key, A, KEY_UP),
                (Kind::Sync, 0, 0)
            ]
        );
    }

    #[test]
    fn the_wheel_goes_to_the_window_under_the_pointer() {
        let mut router = router();
        move_to(&mut router, 20, 20);
        let sent = frame(&mut router, &[rel(generic_desktop::WHEEL, -3)]);
        assert_eq!(
            kinds(&sent, 1),
            [
                (
                    Kind::Rel,
                    code(page::GENERIC_DESKTOP, generic_desktop::WHEEL),
                    -3
                ),
                (Kind::Sync, 0, 0)
            ]
        );
    }

    #[test]
    fn a_frame_with_motion_and_a_press_presses_at_the_new_position() {
        let mut router = router();
        let sent = frame(
            &mut router,
            &[
                key(LEFT, KEY_DOWN),
                rel(generic_desktop::X, -180),
                rel(generic_desktop::Y, -130),
            ],
        );
        assert_eq!(router.pointer(), (20, 20));
        assert_eq!(router.focus(), Some(1));
        assert_eq!(
            kinds(&sent, 1),
            [
                (Kind::Enter, 0, 0),
                x(20),
                y(20),
                (Kind::Focus, 0, 1),
                (Kind::Key, LEFT, KEY_DOWN),
                (Kind::Sync, 0, 0)
            ]
        );
    }

    #[test]
    fn a_device_that_goes_away_releases_what_it_held() {
        let mut router = router();
        let mut out = Output::new();
        router.feed(&Event::new(1, 1, Kind::DeviceAdded, 0, 0), &mut out);
        router.feed(&sync(), &mut out);
        move_to(&mut router, 20, 20);
        frame(&mut router, &[key(LEFT, KEY_DOWN)]);
        frame(&mut router, &[key(A, KEY_DOWN)]);
        out.clear();
        router.feed(&Event::new(9, 1, Kind::DeviceRemoved, 0, 0), &mut out);
        router.feed(&sync(), &mut out);
        let sent = out.items().to_vec();
        let released: Vec<(Kind, u32, i32)> = kinds(&sent, 1)
            .into_iter()
            .filter(|(kind, ..)| *kind == Kind::Key)
            .collect();
        assert_eq!(
            released,
            [(Kind::Key, A, KEY_UP), (Kind::Key, LEFT, KEY_UP)]
        );
        // The pointer button is up: the grab ended.
        move_to(&mut router, 300, 250);
        assert_eq!(router.hover(), None);
    }

    #[test]
    fn dropped_records_mark_the_router_until_a_snapshot_replaces_the_state() {
        let mut router = router();
        let mut out = Output::new();
        router.feed(&Event::new(1, 1, Kind::DeviceAdded, 0, 0), &mut out);
        router.feed(&sync(), &mut out);
        move_to(&mut router, 20, 20);
        frame(&mut router, &[key(LEFT, KEY_DOWN)]);
        frame(&mut router, &[key(A, KEY_DOWN)]);
        router.feed(&Event::new(2, 0, Kind::Dropped, 0, 0), &mut out);
        assert!(router.is_stale());
        // The snapshot says A is up and the left button is still down.
        out.clear();
        router.resync(1, &[key(LEFT, KEY_DOWN), sync()], &mut out);
        assert!(!router.is_stale());
        let sent = out.items().to_vec();
        assert_eq!(
            kinds(&sent, 1),
            [(Kind::Key, A, KEY_UP), (Kind::Sync, 0, 0)],
            "the key the snapshot does not hold is released"
        );
    }

    #[test]
    fn a_window_that_went_away_gets_nothing_and_loses_focus() {
        let mut router = router();
        move_to(&mut router, 20, 20);
        frame(&mut router, &[key(LEFT, KEY_DOWN)]);
        router.forget_window(1);
        assert_eq!(router.focus(), None);
        assert_eq!(router.hover(), None);
        let sent = frame(&mut router, &[key(LEFT, KEY_UP), key(A, KEY_DOWN)]);
        assert!(sent.is_empty());
    }

    #[test]
    fn repeats_and_unknown_pages_change_nothing() {
        let mut router = router();
        move_to(&mut router, 20, 20);
        let sent = frame(&mut router, &[key(LEFT, 2)]);
        assert!(sent.is_empty(), "a repeat of a button is not a press");
        assert_eq!(router.focus(), None);
    }
}

#[cfg(test)]
mod drawing {
    use compositor_core::{
        blit::{Canvas, Layer, Pixels, blend, compose, present},
        cursor,
        damage::{Damage, MAX_DAMAGE},
        windows::Rect,
    };

    const OPAQUE_RED: u32 = 0xffff_0000;
    const BACKGROUND: u32 = 0xff10_2030;

    fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn rectangles_intersect_union_and_touch() {
        let a = rect(0, 0, 10, 10);
        let b = rect(5, 5, 10, 10);
        assert_eq!(a.intersect(&b), Some(rect(5, 5, 5, 5)));
        assert_eq!(
            a.intersect(&rect(10, 0, 5, 5)),
            None,
            "edges do not overlap"
        );
        assert!(a.touches(&rect(10, 0, 5, 5)), "edges touch");
        assert!(!a.touches(&rect(11, 0, 5, 5)));
        assert_eq!(a.union(&b), rect(0, 0, 15, 15));
        assert_eq!(
            a.union(&rect(0, 0, 0, 7)),
            a,
            "an empty rectangle adds nothing"
        );
        assert_eq!(a.intersect(&rect(-5, -5, 7, 7)), Some(rect(0, 0, 2, 2)));
    }

    #[test]
    fn damage_merges_what_touches() {
        let mut damage = Damage::new();
        damage.add(rect(0, 0, 10, 10));
        damage.add(rect(100, 100, 5, 5));
        assert_eq!(damage.rects().len(), 2);
        damage.add(rect(8, 8, 10, 10));
        assert_eq!(damage.rects().len(), 2);
        assert!(damage.rects().contains(&rect(0, 0, 18, 18)));
        // A rectangle that bridges both merges them into one.
        damage.add(rect(10, 10, 92, 92));
        assert_eq!(damage.rects(), &[rect(0, 0, 105, 105)]);
        damage.add(rect(0, 0, 0, 5));
        assert_eq!(damage.rects().len(), 1, "an empty rectangle is no damage");
        damage.clear();
        assert!(damage.is_empty());
    }

    #[test]
    fn damage_overflow_becomes_the_bounding_box() {
        let mut damage = Damage::new();
        for index in 0..MAX_DAMAGE {
            damage.add(rect(i32::try_from(index).expect("a few") * 20, 0, 5, 5));
        }
        assert_eq!(damage.rects().len(), MAX_DAMAGE);
        damage.add(rect(0, 500, 5, 5));
        assert_eq!(damage.rects().len(), 1);
        assert_eq!(damage.rects()[0], rect(0, 0, 145, 505));
    }

    #[test]
    fn blending_follows_premultiplied_alpha() {
        assert_eq!(blend(OPAQUE_RED, BACKGROUND), OPAQUE_RED);
        assert_eq!(blend(0, BACKGROUND), BACKGROUND);
        // Half transparent white over black is half white.
        assert_eq!(blend(0x8080_8080, 0xff00_0000), 0xff80_8080);
        // Half transparent black (all channels zero) halves the destination.
        assert_eq!(blend(0x8000_0000, 0xffff_ffff), 0xff7f_7f7f);
    }

    fn canvas(pixels: &mut [u32], width: u32, height: u32) -> Canvas<'_> {
        Canvas {
            pixels,
            pitch: width as usize,
            width,
            height,
        }
    }

    #[test]
    fn compose_clips_to_the_damage_and_stacks_layers() {
        let mut pixels = [0_u32; 20 * 10];
        let mut canvas = canvas(&mut pixels, 20, 10);
        let lower = [OPAQUE_RED; 8 * 6];
        let upper = [0xff00_ff00_u32; 4 * 4];
        let layers = [
            Layer {
                rect: rect(2, 2, 8, 6),
                pixels: &Pixels(&lower),
                pitch: 8,
            },
            Layer {
                rect: rect(6, 4, 4, 4),
                pixels: &Pixels(&upper),
                pitch: 4,
            },
        ];
        compose(&mut canvas, &rect(0, 0, 12, 5), BACKGROUND, &layers, None);
        let at = |x: usize, y: usize| canvas.pixels[y * 20 + x];
        assert_eq!(at(0, 0), BACKGROUND);
        assert_eq!(at(2, 2), OPAQUE_RED);
        assert_eq!(at(9, 3), OPAQUE_RED);
        assert_eq!(at(6, 4), 0xff00_ff00, "the upper layer covers the lower");
        assert_eq!(at(10, 3), BACKGROUND, "right of the lower layer");
        assert_eq!(at(2, 5), 0, "outside the damage nothing is drawn");
        assert_eq!(at(15, 0), 0, "outside the damage nothing is drawn");
    }

    #[test]
    fn a_layer_that_hangs_off_the_screen_is_clipped() {
        let mut pixels = [0_u32; 10 * 10];
        let mut canvas = canvas(&mut pixels, 10, 10);
        let layer_pixels = [OPAQUE_RED; 6 * 6];
        let layers = [Layer {
            rect: rect(-3, -3, 6, 6),
            pixels: &Pixels(&layer_pixels),
            pitch: 6,
        }];
        compose(
            &mut canvas,
            &rect(-50, -50, 200, 200),
            BACKGROUND,
            &layers,
            None,
        );
        assert_eq!(canvas.pixels.len(), 100);
        assert_eq!(canvas.pixels[0], OPAQUE_RED);
        assert_eq!(canvas.pixels[2 * 10 + 2], OPAQUE_RED);
        assert_eq!(canvas.pixels[3], BACKGROUND);
        assert_eq!(canvas.pixels[3 * 10], BACKGROUND);
    }

    #[test]
    fn the_cursor_is_brightest_on_its_outline_and_darker_inside() {
        // The sum of the colour channels, a stand-in for brightness.
        let light = |pixel: u32| (pixel >> 16 & 0xff) + (pixel >> 8 & 0xff) + (pixel & 0xff);
        // Screen pixels: the art is drawn doubled.
        let outline = cursor::pixel(0, 5);
        let inside = cursor::pixel(2, 5);
        let deeper = cursor::pixel(6, 18);
        assert_eq!(
            cursor::pixel(1, 5),
            outline,
            "each art pixel covers two by two"
        );
        assert_eq!(outline >> 24, 0xff, "opaque");
        assert!(light(outline) > light(inside), "the outline is brightest");
        assert!(light(inside) > light(deeper), "darker towards the middle");
        // Yellow hues: red and green over blue.
        for pixel in [outline, inside, deeper] {
            assert!(pixel >> 16 & 0xff > pixel & 0xff && pixel >> 8 & 0xff > pixel & 0xff);
        }
        assert_eq!(cursor::pixel(17, 0), 0, "outside the arrow");
    }

    #[test]
    fn the_cursor_is_drawn_last_and_clipped() {
        let mut pixels = [0_u32; 30 * 30];
        let mut canvas = canvas(&mut pixels, 30, 30);
        let layer_pixels = [OPAQUE_RED; 30 * 30];
        let layers = [Layer {
            rect: rect(0, 0, 30, 30),
            pixels: &Pixels(&layer_pixels),
            pitch: 30,
        }];
        let everything = rect(0, 0, 30, 30);
        compose(&mut canvas, &everything, BACKGROUND, &layers, Some((5, 5)));
        assert_eq!(canvas.pixels.len(), 900);
        assert_eq!(
            canvas.pixels[5 * 30 + 5],
            cursor::pixel(0, 0),
            "the hot spot"
        );
        assert_eq!(
            canvas.pixels[(5 + 3) * 30 + 5 + 1],
            cursor::pixel(1, 3),
            "inside the arrow"
        );
        assert_eq!(
            canvas.pixels[5 * 30 + 5 + 4],
            OPAQUE_RED,
            "beside the arrow"
        );
        // A cursor half off the screen draws what shows, and one wholly off
        // draws nothing.
        compose(
            &mut canvas,
            &everything,
            BACKGROUND,
            &layers,
            Some((-6, 28)),
        );
        compose(
            &mut canvas,
            &everything,
            BACKGROUND,
            &layers,
            Some((100, 100)),
        );
        assert_eq!(canvas.pixels[0], OPAQUE_RED);
        assert_eq!(cursor::pixel(40, 40), 0);
    }

    #[test]
    fn a_surface_takes_whole_2_mib_frames() {
        use compositor_core::surface::{MAX_FRAMES, frames, pitch_bytes};
        assert_eq!(pitch_bytes(1000), 4000);
        // 1000 x 500 x 4 = 2,000,000 bytes fit one Frame of 2,097,152.
        assert_eq!(frames(1000, 500), Some(1));
        // 3 MiB takes two.
        assert_eq!(frames(1024, 768), Some(2));
        assert_eq!(frames(1024, 512), Some(1));
        assert_eq!(frames(1024, 513), Some(2));
        assert_eq!(frames(0, 10), None);
        assert_eq!(frames(10, 0), None);
        // The largest surface is MAX_FRAMES Frames; one pixel more is refused.
        assert_eq!(frames(2048, 2048), Some(MAX_FRAMES));
        assert_eq!(frames(2048, 2049), None);
        assert_eq!(frames(u32::MAX, u32::MAX), None);
    }

    #[test]
    fn present_copies_only_the_area() {
        let mut pixels = [7_u32; 8 * 4];
        let canvas = canvas(&mut pixels, 8, 4);
        // The display has a wider pitch than the canvas.
        let mut display = [0_u32; 10 * 4];
        present(&canvas, &mut display[..], 10, &rect(2, 1, 3, 2));
        let copied = display.iter().filter(|&&pixel| pixel == 7).count();
        assert_eq!(copied, 6);
        assert_eq!(display.len(), 40);
        assert_eq!(display[10 + 2], 7);
        assert_eq!(display[2 * 10 + 4], 7);
        assert_eq!(display[10 + 5], 0);
        assert_eq!(display[0], 0);
    }
}

/// Host tests of focus on window creation, the client's state snapshot and the
/// client event channel with its drop-newest policy.
#[cfg(test)]
mod client_events {
    use {
        compositor_core::{
            events::{
                BUFFER_BYTES, BUFFERS, EventChannel, Outbox, Receiver, client_records, data_area,
                deliver,
            },
            router::{Output, Routed, Router},
            windows::{Rect, Window},
        },
        libio_channel::{NoEvent, Unwaitable, WaitError},
        vesper_input_event::{
            record::{EVENT_SIZE, Event, KEY_DOWN, KEY_UP, Kind},
            usage::{code, generic_desktop, page},
        },
    };

    const LEFT: u32 = 0x0009_0001;
    const A: u32 = 0x0007_0004;

    fn window(id: u32, client: u32, x: i32, y: i32) -> Window {
        Window {
            id,
            client,
            rect: Rect {
                x,
                y,
                width: 100,
                height: 100,
            },
        }
    }

    fn frame(router: &mut Router, events: &[Event]) -> Vec<Routed> {
        let mut out = Output::new();
        for event in events {
            router.feed(event, &mut out);
        }
        router.feed(&Event::sync(5, 1), &mut out);
        out.items().to_vec()
    }

    fn kinds(routed: &[Routed], window: u32) -> Vec<(Kind, u32, i32)> {
        routed
            .iter()
            .filter(|r| r.window == window)
            .map(|r| (r.event.kind, r.event.code, r.event.value))
            .collect()
    }

    /// Pointer motion by `dx`, `dy`.
    fn motion(dx: i32, dy: i32) -> [Event; 2] {
        let axis =
            |usage, value| Event::new(5, 1, Kind::Rel, code(page::GENERIC_DESKTOP, usage), value);
        [axis(generic_desktop::X, dx), axis(generic_desktop::Y, dy)]
    }

    /// A router with the keyboard as device 1 and windows 1 (client 1, at
    /// 0,0) and 2 (client 2, at 200,0) on a 400x300 screen.
    fn router() -> Router {
        let mut router = Router::new(400, 300);
        let mut out = Output::new();
        router.feed(&Event::new(0, 1, Kind::DeviceAdded, 0, 0), &mut out);
        router.feed(&Event::sync(0, 1), &mut out);
        assert!(router.windows_mut().add(window(1, 1, 0, 0)));
        assert!(router.windows_mut().add(window(2, 2, 200, 0)));
        router
    }

    #[test]
    fn a_new_window_gets_the_focus() {
        let mut router = router();
        let mut out = Output::new();
        router.focus_window(1, 7, &mut out);
        assert_eq!(router.focus(), Some(1));
        assert_eq!(
            kinds(out.items(), 1),
            [(Kind::Focus, 0, 1), (Kind::Sync, 0, 0)]
        );
        // The next window takes it: the old one hears it lost it.
        out.clear();
        router.focus_window(2, 8, &mut out);
        assert_eq!(router.focus(), Some(2));
        assert_eq!(
            kinds(out.items(), 1),
            [(Kind::Focus, 0, 0), (Kind::Sync, 0, 0)]
        );
        assert_eq!(
            kinds(out.items(), 2),
            [(Kind::Focus, 0, 1), (Kind::Sync, 0, 0)]
        );
        assert!(out.items().iter().all(|r| r.event.subject == r.window));
        // Keys go to it.
        let sent = frame(&mut router, &[Event::new(9, 1, Kind::Key, A, KEY_DOWN)]);
        assert_eq!(
            kinds(&sent, 2),
            [(Kind::Key, A, KEY_DOWN), (Kind::Sync, 0, 0)]
        );
        // Focusing the focused window, or one that is not there, sends nothing.
        out.clear();
        router.focus_window(2, 10, &mut out);
        router.focus_window(9, 10, &mut out);
        assert!(out.items().is_empty());
        assert_eq!(router.focus(), Some(2));
    }

    #[test]
    fn a_click_still_moves_the_focus_from_a_new_window() {
        let mut router = router();
        let mut out = Output::new();
        router.focus_window(2, 7, &mut out);
        // The pointer starts at 200,150, outside both; go to window 1 and press.
        frame(&mut router, &motion(-150, -100));
        let sent = frame(&mut router, &[Event::new(5, 1, Kind::Key, LEFT, KEY_DOWN)]);
        assert_eq!(router.focus(), Some(1));
        assert_eq!(kinds(&sent, 2), [(Kind::Focus, 0, 0), (Kind::Sync, 0, 0)]);
        assert_eq!(kinds(&sent, 1)[0], (Kind::Focus, 0, 1));
    }

    #[test]
    fn a_snapshot_rebuilds_focus_keys_and_pointer_for_one_client() {
        let mut router = router();
        let mut out = Output::new();
        router.focus_window(1, 7, &mut out);
        // Into window 1 at 50,50, a key held and the left button held.
        let [dx, dy] = motion(-150, -100);
        frame(
            &mut router,
            &[
                dx,
                dy,
                Event::new(5, 1, Kind::Key, A, KEY_DOWN),
                Event::new(5, 1, Kind::Key, LEFT, KEY_DOWN),
            ],
        );
        let mut records = [Event::sync(0, 0); 16];
        let count = router.snapshot(1, 3, &mut records);
        let got: Vec<(u32, Kind, u32, i32)> = records[..count]
            .iter()
            .map(|event| (event.subject, event.kind, event.code, event.value))
            .collect();
        let x = code(page::GENERIC_DESKTOP, generic_desktop::X);
        let y = code(page::GENERIC_DESKTOP, generic_desktop::Y);
        assert_eq!(
            got,
            [
                (1, Kind::Focus, 0, 1),
                (1, Kind::Key, A, KEY_DOWN),
                (1, Kind::Enter, 0, 0),
                (1, Kind::Abs, x, 50),
                (1, Kind::Abs, y, 50),
                (1, Kind::Key, LEFT, KEY_DOWN),
                (1, Kind::Sync, 0, 0),
            ]
        );
        // The other client has nothing to rebuild.
        assert_eq!(router.snapshot(2, 3, &mut records), 0);
        // Records past the slice are left out.
        assert_eq!(router.snapshot(1, 3, &mut records[..2]), 2);
    }

    #[test]
    fn a_client_receives_only_its_own_windows() {
        let routed = [
            Routed {
                window: 1,
                event: Event::new(1, 1, Kind::Focus, 0, 0),
            },
            Routed {
                window: 1,
                event: Event::sync(1, 1),
            },
            Routed {
                window: 2,
                event: Event::new(1, 2, Kind::Focus, 0, 1),
            },
            Routed {
                window: 2,
                event: Event::sync(1, 2),
            },
            // A window that went away gets nothing.
            Routed {
                window: 9,
                event: Event::sync(1, 9),
            },
        ];
        let records: Vec<Event> = client_records(&routed, router().windows(), 2).collect();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].kind, Kind::Focus);
        assert_eq!(records[0].subject, 2);
        assert_eq!(records[1].kind, Kind::Sync);
    }

    #[test]
    fn an_outbox_drops_the_newest_and_marks_the_next_delivery() {
        let mut outbox = Outbox::new();
        // Two records fit a buffer of 1024 bytes, with no marker.
        assert_eq!(outbox.admit(2, 2, Some(1024)), Some(None));
        assert!(!outbox.missed());
        // No buffer posted: dropped.
        assert_eq!(outbox.admit(2, 3, None), None);
        assert!(outbox.missed());
        // A buffer too small for the delivery: dropped too.
        assert_eq!(outbox.admit(2, 4, Some(24)), None);
        assert_eq!(outbox.dropped(), 2);
        // The next delivery that fits starts with one `Dropped`, stamped.
        let marker = outbox.admit(2, 5, Some(1024)).unwrap().unwrap();
        assert_eq!(
            (marker.kind, marker.subject, marker.timestamp),
            (Kind::Dropped, 0, 5)
        );
        assert!(!outbox.missed());
        assert_eq!(outbox.admit(2, 6, Some(1024)), Some(None), "once");
        // The marker counts toward the size: it must fit with the records.
        assert_eq!(outbox.admit(2, 7, None), None);
        assert_eq!(outbox.admit(2, 8, Some(2 * EVENT_SIZE as u64)), None);
        assert_eq!(
            outbox
                .admit(2, 9, Some(3 * EVENT_SIZE as u64))
                .map(|marker| marker.is_some()),
            Some(true)
        );
        assert_eq!(outbox.dropped(), 4);
    }

    /// A control page and the data page after it, as Kickstart maps them.
    #[repr(C, align(4096))]
    struct Pages([u8; 8192]);

    #[test]
    fn deliveries_reach_the_receiver_and_a_full_channel_drops_the_newest() {
        let pages = Box::new(Pages([0; 8192]));
        let control = (&raw const pages.0).expose_provenance() as u64;
        // SAFETY: the pages are zeroed, aligned and live until the end of the test.
        let channel = unsafe { EventChannel::at(control) };
        let data = data_area(control);
        // SAFETY: the data page is readable for the receiver's life.
        let mut receiver = unsafe { Receiver::new(channel, data, NoEvent) };
        let mut outbox = Outbox::new();
        let key = |stamp: u64, value: i32| {
            [
                Event::new(stamp, 1, Kind::Key, A, value),
                Event::sync(stamp, 1),
            ]
        };
        let send = |outbox: &mut Outbox, records: &[Event], stamp: u64| {
            // SAFETY: the data page is mapped read/write.
            unsafe {
                deliver(
                    channel,
                    data,
                    NoEvent,
                    outbox,
                    records.iter().copied(),
                    stamp,
                )
            }
        };
        // Every posted buffer takes one delivery; the one after them is dropped.
        for stamp in 0..BUFFERS {
            assert!(send(&mut outbox, &key(stamp, KEY_DOWN), stamp));
        }
        assert!(!send(&mut outbox, &key(99, KEY_UP), 99));
        assert_eq!(outbox.dropped(), 1);
        // Nothing for a client is no delivery and loses nothing.
        assert!(send(&mut outbox, &[], 100));
        assert_eq!(outbox.dropped(), 1);

        let delivered: Vec<(u64, Kind)> = core::iter::from_fn(|| receiver.next_event())
            .map(|event| (event.timestamp, event.kind))
            .collect();
        let expected: Vec<(u64, Kind)> = (0..BUFFERS)
            .flat_map(|stamp| [(stamp, Kind::Key), (stamp, Kind::Sync)])
            .collect();
        assert_eq!(delivered, expected, "the dropped delivery never arrives");

        // The receiver posted its buffers again: the next delivery starts with
        // `Dropped`, and the client resyncs from there.
        assert!(send(&mut outbox, &key(101, KEY_UP), 101));
        let kinds: Vec<Kind> = core::iter::from_fn(|| receiver.next_event())
            .map(|event| event.kind)
            .collect();
        assert_eq!(kinds, [Kind::Dropped, Kind::Key, Kind::Sync]);
        assert_eq!(receiver.rejected(), 0);
        assert!(BUFFER_BYTES >= 3 * EVENT_SIZE as u64);
    }

    #[test]
    fn waiting_returns_what_arrived_and_sleeps_only_on_nothing() {
        let pages = Box::new(Pages([0; 8192]));
        let control = (&raw const pages.0).expose_provenance() as u64;
        // SAFETY: the pages are zeroed, aligned and live until the end of the test.
        let channel = unsafe { EventChannel::at(control) };
        let data = data_area(control);
        // SAFETY: the data page is readable for the receiver's life.
        let mut receiver = unsafe { Receiver::new(channel, data, NoEvent) };
        let mut outbox = Outbox::new();
        let focus = [Event::new(1, 1, Kind::Focus, 0, 0), Event::sync(1, 1)];
        // SAFETY: the data page is mapped read/write.
        assert!(unsafe { deliver(channel, data, NoEvent, &mut outbox, focus.into_iter(), 1) });
        let first = receiver.wait_event().expect("a delivery is there");
        assert_eq!((first.kind, first.value), (Kind::Focus, 0));
        assert_eq!(
            receiver.wait_event().map(|event| event.kind),
            Ok(Kind::Sync)
        );
        // With nothing delivered it sleeps on the EventCount, which `NoEvent`
        // refuses.
        assert_eq!(
            receiver.wait_event().map(|event| event.kind),
            Err(WaitError::Event(Unwaitable))
        );
    }
}
