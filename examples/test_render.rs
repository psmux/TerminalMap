use terminalmap::config::MapConfig;
use terminalmap::widget::MapState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = MapConfig::default();
    let mut map = MapState::new(config).await?;

    // Simulate a small terminal: 80 cols x 24 rows
    map.set_size_from_terminal(80, 24);
    map.zoom = 2.0;

    eprintln!("Rendering map at zoom {} ...", map.zoom);
    eprintln!("Center: {}, {}", map.center_lat, map.center_lon);
    eprintln!("Size: {}x{}", map.width, map.height);

    match map.render().await {
        Ok(frame) => {
            eprintln!("Frame length: {} bytes", frame.len());
            print!("{}", frame);
            println!("\n{}", map.footer());
        }
        Err(e) => {
            eprintln!("Render error: {:?}", e);
        }
    }

    Ok(())
}
