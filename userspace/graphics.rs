// fn check_display_init() {
//     display_graphics()
//         .map_err(|e| {
//             warn!("Error in display: {}", e);
//         })
//         .ok();
// }
//
// fn display_graphics() -> Result<(), DrawError> {
//     if let Ok(mut display) = VC::init_fb(800, 600, 32) {
//         info!("Display created");
//
//         display.clear(Color::black());
//         info!("Display cleared");
//
//         display.rect(10, 10, 250, 250, Color::rgb(32, 96, 64));
//         display.draw_text(50, 50, "Hello there!", Color::rgb(128, 192, 255))?;
//
//         let mut buf = [0u8; 64];
//         let s = machine::write_to::show(&mut buf, format_args!("Display width {}", display.width));
//
//         if s.is_err() {
//             display.draw_text(50, 150, "Error displaying", Color::red())?
//         } else {
//             display.draw_text(50, 150, s.unwrap(), Color::white())?
//         }
//
//         display.draw_text(150, 50, "RED", Color::red())?;
//         display.draw_text(160, 60, "GREEN", Color::green())?;
//         display.draw_text(170, 70, "BLUE", Color::blue())?;
//     }
//     Ok(())
// }
