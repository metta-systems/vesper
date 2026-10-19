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
