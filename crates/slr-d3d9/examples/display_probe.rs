//! Real native-window diagnostic with authored pixels; no original-scene claim.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use slr_d3d9::{Frame, Image, Message, Vertex, WindowOptions};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
fn verify(output: &Path) -> Result<Value, String> {
    let text =
        std::fs::read_to_string(output.join("session-00000.jsonl")).map_err(|e| e.to_string())?;
    let rows = text
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut images = Vec::new();
    for event in rows.iter().filter(|e| e["kind"] == "drawable-captured") {
        let d = &event["data"];
        let file = d["file"].as_str().ok_or("capture filename")?;
        if Path::new(file).components().count() != 1 {
            return Err("capture path escapes bundle".into());
        }
        let path = output.join(file);
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let mut reader = png::Decoder::new(std::io::Cursor::new(&bytes))
            .read_info()
            .map_err(|e| e.to_string())?;
        let mut pixels = vec![0; reader.output_buffer_size().ok_or("PNG buffer")?];
        let info = reader.next_frame(&mut pixels).map_err(|e| e.to_string())?;
        if info.color_type != png::ColorType::Rgba
            || info.bit_depth != png::BitDepth::Eight
            || u64::from(info.width) != d["width"].as_u64().ok_or("capture width")?
            || u64::from(info.height) != d["height"].as_u64().ok_or("capture height")?
        {
            return Err("PNG geometry/format differs from capture".into());
        }
        let rgba = &pixels[..info.buffer_size()];
        let raw_sha = format!("{:x}", Sha256::digest(rgba));
        if d["rgba_sha256"] != raw_sha {
            return Err("PNG pixels differ from captured pixels".into());
        }
        let number = |key: &str| {
            d["letterbox"][key]
                .as_u64()
                .ok_or_else(|| format!("letterbox {key}"))
        };
        let (x, y, w, h) = (number("x")?, number("y")?, number("w")?, number("h")?);
        if w < 16 || h < 16 || x + w > u64::from(info.width) || y + h > u64::from(info.height) {
            return Err("invalid capture letterbox".into());
        }
        let frame = d["frame_id"].as_u64().ok_or("frame ID")?;
        if frame == 0 {
            return Err("no authored frame presented".into());
        }
        let inside = if frame < 60 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        };
        for py in 0..u64::from(info.height) {
            for px in 0..u64::from(info.width) {
                let expected = if px >= x && px < x + w && py >= y && py < y + h {
                    inside
                } else {
                    [0, 0, 0, 255]
                };
                let at = ((py * u64::from(info.width) + px) * 4) as usize;
                if rgba[at..at + 4] != expected {
                    return Err(format!("authored drawable pixel mismatch at {px},{py}"));
                }
            }
        }
        images.push(json!({"name":d["name"],"file":file,"png_sha256":format!("{:x}",Sha256::digest(&bytes)),"rgba_sha256":raw_sha,"width":info.width,"height":info.height,"authored_pixels_match":true}));
    }
    if images.len() != 3 {
        return Err(format!("expected three captures, got {}", images.len()));
    }
    Ok(
        json!({"schema":1,"images":images,"authored_inputs":true,"stock_or_original_assets_used":false}),
    )
}
fn main() -> Result<(), String> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: display_probe <new-output-directory>")?;
    slr_telemetry::journal::configure(&output, "authored-native-display-probe")?;
    slr_telemetry::journal::emit(
        "display-probe-start",
        json!({"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"authored_inputs":true,"stock_or_original_assets_used":false}),
    )?;
    let options = WindowOptions::from_env()?;
    slr_d3d9::window_with(options, move |sender, inputs| {
        for id in 1..1000 {
            let color = if id < 60 {
                [1., 0., 0., 1.]
            } else {
                [0., 0., 1., 1.]
            };
            let mut frame = Frame {
                id,
                ..Default::default()
            };
            frame.images.insert(
                "authored-flat".into(),
                Image {
                    width: 1,
                    height: 1,
                    rgba: Arc::new(vec![255; 4]),
                },
            );
            let vertex = |position, uv| Vertex {
                position,
                uv,
                color,
            };
            frame.draws.push((
                "authored-flat".into(),
                vec![
                    vertex([-1., 1.], [0., 0.]),
                    vertex([1., 1.], [1., 0.]),
                    vertex([1., -1.], [1., 1.]),
                    vertex([-1., 1.], [0., 0.]),
                    vertex([1., -1.], [1., 1.]),
                    vertex([-1., -1.], [0., 1.]),
                ],
            ));
            if sender.send(Message::Frame(Box::new(frame))).is_err() {
                break;
            }
            while inputs.try_recv().is_ok() {}
            std::thread::sleep(Duration::from_millis(16));
        }
    })?;
    let report = verify(&output)?;
    std::fs::write(
        output.join("pixels.json"),
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("{report}");
    Ok(())
}
