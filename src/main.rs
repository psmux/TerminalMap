use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, Write};

use terminalmap::config::MapConfig;
use terminalmap::widget::MapState;

fn add_demo_markers(map: &mut MapState) {
    use terminalmap::marker::{MapMarker, MarkerAnimation, MarkerShape};
    map.add_marker(
        MapMarker::dot_rgb(52.52, 13.405, 255, 50, 50)
            .with_label("Berlin")
            .with_animation(MarkerAnimation::Blink)
            .with_id("berlin"),
    );
    map.add_marker(
        MapMarker::dot_rgb(48.8566, 2.3522, 50, 200, 255)
            .with_label("Paris")
            .with_animation(MarkerAnimation::Pulse)
            .with_shape(MarkerShape::Ring(4))
            .with_id("paris"),
    );
    map.add_marker(
        MapMarker::dot_rgb(51.5074, -0.1278, 255, 255, 50)
            .with_label("London")
            .with_animation(MarkerAnimation::Flash)
            .with_shape(MarkerShape::Diamond)
            .with_id("london"),
    );
    map.add_marker(
        MapMarker::dot_rgb(41.9028, 12.4964, 50, 255, 50)
            .with_label("Rome")
            .with_shape(MarkerShape::Cross)
            .with_id("rome"),
    );
    map.add_marker(
        MapMarker::dot_rgb(40.4168, -3.7038, 255, 165, 0)
            .with_label("Madrid")
            .with_shape(MarkerShape::FilledCircle(3))
            .with_id("madrid"),
    );
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = MapConfig::default();
    let mut map = MapState::new(config).await?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;

    // Get terminal size and initialize
    let (cols, rows) = crossterm::terminal::size()?;
    map.set_size_from_terminal(cols, rows);

    // Initial draw
    draw_map(&map, &mut stdout).await?;
    print_footer(&map, &mut stdout)?;

    // Main event loop
    let mut last_draw = std::time::Instant::now();
    loop {
        if event::poll(std::time::Duration::from_millis(50))? {
            let mut needs_redraw = false;

            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char('a') | KeyCode::Char('+') => {
                            map.zoom_by(map.config.zoom_step);
                            needs_redraw = true;
                        }
                        KeyCode::Char('z') | KeyCode::Char('y') | KeyCode::Char('-') => {
                            map.zoom_by(-map.config.zoom_step);
                            needs_redraw = true;
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            map.move_by(0.0, -8.0 / 2.0_f64.powf(map.zoom));
                            needs_redraw = true;
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            map.move_by(0.0, 8.0 / 2.0_f64.powf(map.zoom));
                            needs_redraw = true;
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            map.move_by(6.0 / 2.0_f64.powf(map.zoom), 0.0);
                            needs_redraw = true;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            map.move_by(-6.0 / 2.0_f64.powf(map.zoom), 0.0);
                            needs_redraw = true;
                        }
                        KeyCode::Char('c') => {
                            map.toggle_braille();
                            needs_redraw = true;
                        }
                        KeyCode::Char('n') => {
                            map.toggle_labels();
                            needs_redraw = true;
                        }
                        KeyCode::Char('o') => {
                            map.toggle_ocean_background();
                            needs_redraw = true;
                        }
                        KeyCode::Char('w') => {
                            map.fit_world();
                            needs_redraw = true;
                        }
                        KeyCode::Char('g') => {
                            // Toggle globe tour
                            if map.camera().is_active() {
                                map.camera_mut().stop();
                            } else {
                                map.start_globe_tour();
                            }
                            needs_redraw = true;
                        }
                        KeyCode::Char('t') => {
                            // Toggle marker tour
                            if map.camera().is_active() {
                                map.camera_mut().stop();
                            } else {
                                map.start_marker_tour(5.0);
                            }
                            needs_redraw = true;
                        }
                        KeyCode::Char('m') => {
                            if map.markers().is_empty() {
                                add_demo_markers(&mut map);
                            } else {
                                map.clear_markers();
                            }
                            needs_redraw = true;
                        }
                        _ => {}
                    }
                }
                Event::Mouse(mouse_event) => {
                    match mouse_event.kind {
                        MouseEventKind::ScrollUp => {
                            map.zoom_by(map.config.zoom_step);
                            needs_redraw = true;
                        }
                        MouseEventKind::ScrollDown => {
                            map.zoom_by(-map.config.zoom_step);
                            needs_redraw = true;
                        }
                        _ => {}
                    }
                }
                Event::Resize(cols, rows) => {
                    map.set_size_from_terminal(cols, rows);
                    needs_redraw = true;
                }
                _ => {}
            }

            if needs_redraw {
                draw_map(&map, &mut stdout).await?;
                print_footer(&map, &mut stdout)?;
                last_draw = std::time::Instant::now();
            }
        }

        // Advance animation tick and redraw for animated markers/camera (~200ms refresh)
        map.advance_tick();
        let camera_moved = map.update_camera();
        if (map.needs_animation_redraw() || camera_moved)
            && last_draw.elapsed() >= std::time::Duration::from_millis(50)
        {
            draw_map(&map, &mut stdout).await?;
            print_footer(&map, &mut stdout)?;
            last_draw = std::time::Instant::now();
        }
    }

    // Cleanup
    execute!(
        stdout,
        crossterm::event::DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    disable_raw_mode()?;

    Ok(())
}

async fn draw_map(map: &MapState, stdout: &mut io::Stdout) -> anyhow::Result<()> {
    let frame = map.render().await?;
    // Move cursor to top-left and write the frame
    execute!(stdout, crossterm::cursor::MoveTo(0, 0))?;
    write!(stdout, "{}", frame)?;
    stdout.flush()?;
    Ok(())
}

fn print_footer(map: &MapState, stdout: &mut io::Stdout) -> anyhow::Result<()> {
    let (_, rows) = crossterm::terminal::size()?;

    // Row 1: help bar
    let help = "\x1B[90m arrows/hjkl:\x1B[37mPan  \x1B[90ma/z:\x1B[37mZoom  \x1B[90mc:\x1B[37mBraille  \x1B[90mn:\x1B[37mLabels  \x1B[90mo:\x1B[37mOcean  \x1B[90mm:\x1B[37mMarkers  \x1B[90mg:\x1B[37mGlobe  \x1B[90mt:\x1B[37mTour  \x1B[90mw:\x1B[37mWorld  \x1B[90mq:\x1B[37mQuit\x1B[0m";
    execute!(stdout, crossterm::cursor::MoveTo(0, rows - 2))?;
    write!(stdout, "\x1B[K{}", help)?;

    // Row 2: status
    let mut status = map.footer();
    if let Some(label) = map.camera().current_label() {
        status.push_str(&format!("   >> {}", label));
    }
    if map.camera().is_active() {
        status.push_str("   [TOUR: g/t to stop]");
    }
    execute!(stdout, crossterm::cursor::MoveTo(0, rows - 1))?;
    write!(stdout, "\x1B[K{}", status)?;
    stdout.flush()?;
    Ok(())
}

