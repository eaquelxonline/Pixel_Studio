//! Developer tool: replays a JSON-lines command script and writes every frame as PNG.
//!
//! `cargo run --example run_script -- <script.jsonl> <out-dir> [width height] [scale]`

use pixcore::Engine;

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
    for line in script.lines().filter(|l| !l.trim().is_empty()) {
        let r = engine.execute(line.as_bytes(), &[]);
        if !r.json.contains("\"ok\":true") {
            eprintln!("FAILED: {line}\n  -> {}", r.json);
            std::process::exit(1);
        }
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
