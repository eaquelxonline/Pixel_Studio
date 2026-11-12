//! Developer tool: replays a JSON-lines command script through the engine.
//!
//! `cargo run --release --example run_script -- <script.jsonl> <out-dir> [width height] [scale]`
//!
//! Any command line may carry `"_save": "file.ext"`; its binary output (PNG, GIF, .pxs, …) is
//! written to the output directory, and its JSON result next to it when `"_json": true`.
//! Without explicit saves, every 2D frame is exported as PNG plus an animated GIF and the project.

use pixcore::Engine;
use pixcore::commands::json::Json;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: run_script <script.jsonl> <out-dir> [width height] [scale]");
        std::process::exit(2);
    }
    let w = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(32);
    let h = args.get(4).and_then(|v| v.parse().ok()).unwrap_or(32);
    let scale: u32 = args.get(5).and_then(|v| v.parse().ok()).unwrap_or(8);
    let script = std::fs::read_to_string(&args[1]).expect("read script");
    let out = std::path::Path::new(&args[2]);
    std::fs::create_dir_all(out).expect("create out dir");
    let mut engine = Engine::new(w, h);
    let mut saved_any = false;
    let started = std::time::Instant::now();
    for (n, line) in script.lines().enumerate().filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with("//")) {
        let t = std::time::Instant::now();
        let r = engine.execute(line.as_bytes(), &[]);
        if !r.json.contains("\"ok\":true") {
            eprintln!("FAILED line {}: {line}\n  -> {}", n + 1, r.json);
            std::process::exit(1);
        }
        if let Ok(req) = Json::parse(line)
            && let Some(name) = req.get("_save").and_then(Json::as_str)
        {
            let file = out.join(name);
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir).expect("create dir");
            }
            std::fs::write(&file, &r.binary).expect("write output");
            if req.get("_json").and_then(Json::as_bool) == Some(true) {
                let result = Json::parse(&r.json).ok().and_then(|j| j.get("result").cloned()).unwrap_or(Json::Null);
                std::fs::write(file.with_extension("json"), result.to_json_string()).expect("write json");
            }
            println!(
                "[{:>7.2}s] {name} ({} bytes, {:.0} ms)",
                started.elapsed().as_secs_f32(),
                r.binary.len(),
                t.elapsed().as_secs_f32() * 1000.0
            );
            saved_any = true;
        }
    }
    if saved_any {
        return;
    }
    for f in 0..engine.document().frames.len() {
        let req = format!("{{\"op\":\"export_png\",\"frame\":{f},\"scale\":{scale}}}");
        let r = engine.execute(req.as_bytes(), &[]);
        std::fs::write(out.join(format!("frame_{f:03}.png")), &r.binary).expect("write png");
    }
    let gif = engine.execute(format!("{{\"op\":\"export_gif\",\"scale\":{scale}}}").as_bytes(), &[]);
    std::fs::write(out.join("animation.gif"), &gif.binary).expect("write gif");
    let pxs = engine.execute(b"{\"op\":\"project_save\"}", &[]);
    std::fs::write(out.join("project.pxs"), &pxs.binary).expect("write pxs");
    println!("{} frames written to {}", engine.document().frames.len(), out.display());
}
