use super::*;

fn run(e: &mut Engine, req: &str) -> Json {
    let r = e.execute(req.as_bytes(), &[]);
    Json::parse(&r.json).unwrap()
}

fn ok(e: &mut Engine, req: &str) -> Json {
    let j = run(e, req);
    assert_eq!(j.get("ok"), Some(&Json::Bool(true)), "request {req} failed: {}", j.to_json_string());
    j
}

fn err_code(e: &mut Engine, req: &str) -> String {
    let j = run(e, req);
    assert_eq!(j.get("ok"), Some(&Json::Bool(false)), "request {req} unexpectedly succeeded");
    j.get("error").unwrap().get("code").unwrap().as_str().unwrap().to_string()
}

#[test]
fn pixel_lands_on_exact_coordinate() {
    let mut e = Engine::new(32, 32);
    ok(&mut e, r##"{"op":"set_pixel","actor":"claude","args":{"x":12,"y":18,"color":"#D94B42"}}"##);
    let p = ok(&mut e, r#"{"op":"get_pixel","x":12,"y":18}"#);
    assert_eq!(p.get("result").unwrap().get("color").unwrap().as_str(), Some("#D94B42"));
    let n = ok(&mut e, r#"{"op":"get_pixel","x":13,"y":18}"#);
    assert_eq!(n.get("result").unwrap().get("transparent"), Some(&Json::Bool(true)));
    assert_eq!(e.revision(), 1);
    assert_eq!(err_code(&mut e, r##"{"op":"set_pixel","x":32,"y":0,"color":"#fff"}"##), "invalid_argument");
}

#[test]
fn undo_restores_previous_image_and_redo_reapplies() {
    let mut e = Engine::new(16, 16);
    ok(&mut e, r##"{"op":"draw_rectangle","x":2,"y":2,"width":5,"height":5,"color":"#00FF00","filled":true}"##);
    let before = crate::renderer::render(e.document(), 0, &Default::default());
    ok(&mut e, r##"{"op":"fill_region","x":0,"y":0,"color":"#0000FF"}"##);
    assert_ne!(crate::renderer::render(e.document(), 0, &Default::default()), before);
    ok(&mut e, r#"{"op":"history_undo"}"#);
    assert_eq!(crate::renderer::render(e.document(), 0, &Default::default()), before);
    ok(&mut e, r#"{"op":"history_redo"}"#);
    assert_ne!(e.document().cel(e.document().active_layer, e.document().active_frame).unwrap().get(0, 0), 0);
    assert_eq!(err_code(&mut e, r#"{"op":"history_redo"}"#), "nothing_to_do");
}

#[test]
fn stale_revision_cannot_overwrite_newer_work() {
    let mut e = Engine::new(8, 8);
    // Human and agent both read revision 0.
    ok(&mut e, r##"{"op":"set_pixel","actor":"human","expectedRevision":0,"x":1,"y":1,"color":"#FFFFFF"}"##);
    let code = err_code(&mut e, r##"{"op":"set_pixel","actor":"agent","expectedRevision":0,"x":1,"y":1,"color":"#000000"}"##);
    assert_eq!(code, "conflict");
    let p = ok(&mut e, r#"{"op":"get_pixel","x":1,"y":1}"#);
    assert_eq!(p.get("result").unwrap().get("color").unwrap().as_str(), Some("#FFFFFF"));
}

#[test]
fn operation_ids_are_idempotent() {
    let mut e = Engine::new(8, 8);
    let req = r#"{"op":"frame_add","operationId":"op_1","args":{"count":3}}"#;
    ok(&mut e, req);
    let replay = ok(&mut e, req);
    assert_eq!(replay.get("replayed"), Some(&Json::Bool(true)));
    assert_eq!(e.document().frames.len(), 4);
    assert_eq!(e.revision(), 1);
}

#[test]
fn transactions_are_atomic() {
    let mut e = Engine::new(8, 8);
    let bad = r##"{"op":"transaction","args":{"commands":[
        {"op":"set_pixel","x":0,"y":0,"color":"#FF0000"},
        {"op":"set_pixel","x":99,"y":0,"color":"#FF0000"}]}}"##;
    assert_eq!(err_code(&mut e, bad), "invalid_argument");
    assert_eq!(e.revision(), 0);
    assert!(e.document().cel(e.document().active_layer, e.document().active_frame).is_none_or(|g| g.is_empty()));
    let good = r##"{"op":"transaction","args":{"commands":[
        {"op":"set_pixel","x":0,"y":0,"color":"#FF0000"},
        {"op":"draw_line","args":{"x0":0,"y0":7,"x1":7,"y1":7,"color":"#00FF00"}}]}}"##;
    let j = ok(&mut e, good);
    assert_eq!(j.get("result").unwrap().get("changedPixels").unwrap().as_i64(), Some(9));
    assert_eq!(e.revision(), 1);
    ok(&mut e, r#"{"op":"history_undo"}"#);
    assert!(e.document().cel(e.document().active_layer, e.document().active_frame).is_none_or(|g| g.is_empty()));
}

#[test]
fn dynamic_frames_and_durations() {
    let mut e = Engine::new(16, 16);
    ok(&mut e, r#"{"op":"frame_add","count":46}"#);
    assert_eq!(e.document().frames.len(), 47);
    ok(&mut e, r#"{"op":"frame_set_duration","frame":0,"duration":120}"#);
    ok(&mut e, r#"{"op":"frame_set_duration","frame":1,"duration":80}"#);
    ok(&mut e, r#"{"op":"animation_tag_create","name":"Walk","from":0,"to":3,"direction":"pingpong"}"#);
    let info = ok(&mut e, r#"{"op":"animation_info","tag":"Walk"}"#);
    let seq = info.get("result").unwrap().get("sequence").unwrap().as_array().unwrap();
    let frames: Vec<i64> = seq.iter().map(|s| s.get("frame").unwrap().as_i64().unwrap()).collect();
    assert_eq!(frames, vec![0, 1, 2, 3, 2, 1]);
    assert_eq!(info.get("result").unwrap().get("totalMs").unwrap().as_i64(), Some(120 + 80 + 100 + 100 + 100 + 80));
    ok(&mut e, r#"{"op":"frame_delete","frame":46}"#);
    assert_eq!(err_code(&mut e, r#"{"op":"frame_delete"}"#), "invalid_argument");
}

#[test]
fn layers_isolate_edits() {
    let mut e = Engine::new(16, 16);
    ok(&mut e, r#"{"op":"layer_rename","layer":0,"name":"Body"}"#);
    ok(&mut e, r#"{"op":"layer_create","name":"Weapon"}"#);
    ok(&mut e, r##"{"op":"draw_line","layer":"Weapon","x0":10,"y0":2,"x1":10,"y1":12,"color":"#CCCCCC"}"##);
    ok(&mut e, r##"{"op":"fill_region","layer":"Body","x":0,"y":0,"color":"#884422"}"##);
    let r = ok(&mut e, r#"{"op":"read_region","layer":"Weapon","x":9,"y":1,"width":3,"height":3}"#);
    let rows = r.get("result").unwrap().get("rows").unwrap().as_array().unwrap();
    assert_eq!(rows[0].as_str(), Some("..."));
    assert_eq!(rows[1].as_str(), Some(".0."));
    ok(&mut e, r#"{"op":"layer_lock","layer":"Weapon","locked":true}"#);
    assert_eq!(err_code(&mut e, r##"{"op":"set_pixel","layer":"Weapon","x":0,"y":0,"color":"#000"}"##), "locked");
    let c = ok(&mut e, r#"{"op":"get_pixel","x":10,"y":5,"composite":true}"#);
    assert_eq!(c.get("result").unwrap().get("color").unwrap().as_str(), Some("#CCCCCC"));
}

#[test]
fn copy_between_frames_builds_animation() {
    let mut e = Engine::new(16, 16);
    ok(&mut e, r##"{"op":"draw_rectangle","x":1,"y":1,"width":4,"height":4,"color":"#FF00FF","filled":true}"##);
    ok(&mut e, r#"{"op":"frame_add"}"#);
    ok(&mut e, r#"{"op":"copy_region","frame":0,"x":0,"y":0,"width":8,"height":8,"toFrame":1,"toX":2,"toY":0}"#);
    let p = ok(&mut e, r#"{"op":"get_pixel","frame":1,"x":3,"y":1}"#);
    assert_eq!(p.get("result").unwrap().get("color").unwrap().as_str(), Some("#FF00FF"));
    let p = ok(&mut e, r#"{"op":"get_pixel","frame":1,"x":1,"y":1}"#);
    assert_eq!(p.get("result").unwrap().get("transparent"), Some(&Json::Bool(true)));
}

#[test]
fn save_and_load_through_engine() {
    let mut e = Engine::new(24, 24);
    ok(&mut e, r##"{"op":"draw_ellipse","x":2,"y":2,"width":20,"height":20,"color":"#3366FF","filled":true}"##);
    ok(&mut e, r#"{"op":"frame_duplicate"}"#);
    let saved = e.execute(br#"{"op":"project_save"}"#, &[]);
    assert!(saved.binary.starts_with(b"PXS\0"));
    let mut other = Engine::new(1, 1);
    let r = other.execute(br#"{"op":"project_load"}"#, &saved.binary);
    assert!(r.json.contains("\"ok\":true"), "{}", r.json);
    for f in 0..2 {
        assert_eq!(
            crate::renderer::render(other.document(), f, &Default::default()),
            crate::renderer::render(e.document(), f, &Default::default())
        );
    }
    assert!(other.revision() >= 2);
    let bad = other.execute(br#"{"op":"project_load"}"#, b"garbage");
    assert!(bad.json.contains("invalid_file"));
}

#[test]
fn exports_produce_binary() {
    let mut e = Engine::new(8, 8);
    ok(&mut e, r##"{"op":"set_pixel","x":0,"y":0,"color":"#FF0000"}"##);
    ok(&mut e, r#"{"op":"frame_add","count":2}"#);
    let png = e.execute(br#"{"op":"export_png","scale":4}"#, &[]);
    assert!(png.binary.starts_with(&[0x89, b'P', b'N', b'G']));
    let gif = e.execute(br#"{"op":"export_gif"}"#, &[]);
    assert!(gif.binary.starts_with(b"GIF89a"));
    let sheet = e.execute(br#"{"op":"export_spritesheet","columns":3}"#, &[]);
    let meta = Json::parse(&sheet.json).unwrap();
    assert_eq!(meta.get("result").unwrap().get("image").unwrap().get("width").unwrap().as_i64(), Some(24));
    let raw = e.execute(br#"{"op":"render_frame","scale":2}"#, &[]);
    assert_eq!(raw.binary.len(), 16 * 16 * 4);
    let snap = e.execute(br#"{"op":"canvas_snapshot"}"#, &[]);
    assert!(snap.binary.starts_with(&[0x89, b'P']));
    assert!(snap.json.contains("usedColors"));
}

#[test]
fn hostile_input_never_panics() {
    let mut e = Engine::new(8, 8);
    let inputs: [&[u8]; 12] = [
        b"",
        b"\xff\xfe",
        b"[]",
        b"{}",
        b"{\"op\":42}",
        b"{\"op\":\"nope\"}",
        b"{\"op\":\"set_pixel\",\"x\":1e300,\"y\":0,\"color\":\"#fff\"}",
        b"{\"op\":\"set_pixels\",\"pixels\":[[1],[\"a\",\"b\",\"c\"]]}",
        b"{\"op\":\"draw_line\",\"x0\":-999999,\"y0\":-999999,\"x1\":999999,\"y1\":999999,\"color\":\"#fff\"}",
        b"{\"op\":\"read_region\",\"width\":100000,\"height\":100000}",
        b"{\"op\":\"export_png\",\"scale\":64}",
        b"{\"op\":\"transaction\",\"commands\":[{\"op\":\"history_undo\"}]}",
    ];
    for i in inputs {
        let r = e.execute(i, &[]);
        assert!(Json::parse(&r.json).is_ok());
    }
}

/// Cross-platform golden test: the shared scene script must render to the shared fingerprint.
/// The Kotlin/JNI test suite replays the same files and checks the same value.
#[test]
fn golden_scene_fingerprint() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/golden");
    let script = std::fs::read_to_string(root.join("scene.jsonl")).expect("golden scene");
    let expected = std::fs::read_to_string(root.join("scene.fingerprint")).expect("golden fingerprint");
    let mut e = Engine::new(32, 32);
    for line in script.lines().filter(|l| !l.trim().is_empty()) {
        let r = e.execute(line.as_bytes(), &[]);
        assert!(r.json.contains("\"ok\":true"), "golden step failed: {line} -> {}", r.json);
    }
    let mut prints = Vec::new();
    for f in 0..e.document().frames.len() {
        prints.push(format!("{:016x}", crate::renderer::render(e.document(), f, &Default::default()).fingerprint()));
    }
    assert_eq!(prints.join("\n"), expected.trim(), "golden fingerprint changed");
}

#[test]
fn voxel_pipeline_model_rig_animate_bake_save() {
    let mut e = Engine::new(32, 32);
    ok(&mut e, r#"{"op":"voxel_model_create","name":"Bot","width":48,"height":64,"depth":48}"#);
    ok(&mut e, r#"{"op":"texture_generate","name":"steel","kind":"brushed_metal","size":32,"metallic":0.9}"#);
    for bone in [
        r#"{"op":"rig_bone_add","name":"hips","head":[24,30,24],"tail":[24,36,24]}"#,
        r#"{"op":"rig_bone_add","name":"chest","parent":"hips","head":[24,36,24],"tail":[24,52,24]}"#,
        r#"{"op":"rig_bone_add","name":"head","parent":"chest","head":[24,52,24],"tail":[24,62,24]}"#,
        r#"{"op":"rig_bone_add","name":"upper_leg_l","parent":"hips","head":[28,30,24],"tail":[28,15,24]}"#,
        r#"{"op":"rig_bone_add","name":"lower_leg_l","parent":"upper_leg_l","head":[28,15,24],"tail":[28,1,24]}"#,
        r#"{"op":"rig_bone_add","name":"upper_leg_r","parent":"hips","head":[20,30,24],"tail":[20,15,24]}"#,
        r#"{"op":"rig_bone_add","name":"lower_leg_r","parent":"upper_leg_r","head":[20,15,24],"tail":[20,1,24]}"#,
    ] {
        ok(&mut e, bone);
    }
    ok(
        &mut e,
        r##"{"op":"voxel_shape","shape":"box","min":[17,30,19],"max":[31,51,29],"color":"#8899AA","material":"steel","bone":"chest"}"##,
    );
    ok(&mut e, r##"{"op":"voxel_shape","shape":"ellipsoid","center":[24,57,24],"radius":5,"color":"#F0C090","bone":"head","jitter":0.3}"##);
    ok(
        &mut e,
        r##"{"op":"voxel_shape","shape":"capsule","a":[28,29,24],"b":[28,2,24],"radius":3,"color":"#334455","color_to":"#556677","bone":"upper_leg_l","mirror_x":true}"##,
    );
    ok(&mut e, r#"{"op":"rig_bind","only_unbound":true}"#);
    let generated = ok(&mut e, r#"{"op":"anim3d_generate","kind":"walk","frames":8}"#);
    assert_eq!(generated.get("result").unwrap().get("frames").unwrap().as_i64(), Some(8));
    let mirrored = ok(&mut e, r#"{"op":"voxel_get","x":19,"y":10,"z":24}"#);
    assert_eq!(mirrored.get("result").unwrap().get("bone").unwrap().as_str(), Some("upper_leg_r"), "mirror swaps _l/_r bones");

    let img = e.execute(br#"{"op":"voxel_render","yaw":45,"width":96,"animation":"walk","frame":2,"outline":true,"supersample":2}"#, &[]);
    assert!(img.json.contains("\"ok\":true"), "{}", img.json);
    let decoded = crate::export::png::decode(&img.binary).unwrap();
    assert!(decoded.pixels.chunks(4).filter(|p| p[3] > 0).count() > 200);
    let normal = e.execute(br#"{"op":"voxel_render","pass":"normal","width":64}"#, &[]);
    assert!(normal.json.contains("\"ok\":true"));
    let sheet = e.execute(br#"{"op":"voxel_render_sheet","animation":"walk","directions":8,"width":48,"pass":"albedo"}"#, &[]);
    let sheet_img = crate::export::png::decode(&sheet.binary).unwrap();
    assert_eq!((sheet_img.width, sheet_img.height), (48 * 8, 48 * 8));

    let baked = ok(&mut e, r#"{"op":"voxel_render_to_frames","animation":"walk","directions":8,"width":64,"height":64,"outline":true}"#);
    let r = baked.get("result").unwrap();
    assert_eq!(r.get("framesAdded").unwrap().as_i64(), Some(64));
    assert_eq!(r.get("tags").unwrap().as_array().unwrap().len(), 8);
    assert_eq!(e.document().frames.len(), 64, "the empty starting frame is reused");
    assert_eq!((e.document().width, e.document().height), (64, 64));
    let info = ok(&mut e, r#"{"op":"animation_info","tag":"walk_NE"}"#);
    assert_eq!(info.get("result").unwrap().get("sequence").unwrap().as_array().unwrap().len(), 8);

    let pick = ok(&mut e, r#"{"op":"voxel_pick","yaw":0,"pitch":0,"width":96,"px":48,"py":30}"#);
    assert_eq!(pick.get("result").unwrap().get("hit"), Some(&Json::Bool(true)));

    let saved = e.execute(br#"{"op":"project_save"}"#, &[]);
    let mut other = Engine::new(1, 1);
    assert!(other.execute(br#"{"op":"project_load"}"#, &saved.binary).json.contains("\"ok\":true"));
    let a = e.document();
    let b = other.document();
    assert_eq!(a.models[0].count(), b.models[0].count());
    assert_eq!(a.models[0].bones, b.models[0].bones);
    assert_eq!(a.models[0].animations, b.models[0].animations);
    assert_eq!(a.materials, b.materials);
    assert_eq!(a.textures.len(), b.textures.len());
    let again =
        other.execute(br#"{"op":"voxel_render","yaw":45,"width":96,"animation":"walk","frame":2,"outline":true,"supersample":2}"#, &[]);
    assert_eq!(again.binary, img.binary, "a loaded project renders identically");

    // Undo the bake restores the 2D project exactly.
    ok(&mut e, r#"{"op":"history_undo"}"#);
    assert_eq!(e.document().frames.len(), 1);
}

#[test]
fn true_color_and_png_import() {
    let mut e = Engine::new(8, 8);
    // Two colors that differ by one bit in one channel are both kept exactly.
    ok(&mut e, r##"{"op":"set_pixels","pixels":[[0,0,"#102030"],[1,0,"#102031"],[2,0,"#10203080"]]}"##);
    let r = ok(&mut e, r#"{"op":"read_region","x":0,"y":0,"width":3,"height":1,"format":"hex"}"#);
    let row = r.get("result").unwrap().get("rows").unwrap().as_array().unwrap()[0].as_array().unwrap().to_vec();
    assert_eq!(row, vec![Json::from("#102030"), Json::from("#102031"), Json::from("#10203080")]);
    let png = e.execute(br#"{"op":"export_png"}"#, &[]).binary;
    ok(&mut e, r#"{"op":"layer_create","name":"Imported"}"#);
    let imp = e.execute(br#"{"op":"image_import","layer":"Imported","x":4,"y":4}"#, &png);
    assert!(imp.json.contains("\"ok\":true"), "{}", imp.json);
    let p = ok(&mut e, r#"{"op":"get_pixel","layer":"Imported","x":5,"y":4}"#);
    assert_eq!(p.get("result").unwrap().get("color").unwrap().as_str(), Some("#102031"));
    // Swatch edit recolors the art that uses the old color.
    ok(&mut e, r##"{"op":"palette_add","colors":["#102030"]}"##);
    let idx = e.document().palette.find(crate::pixel::Rgba::parse_hex("#102030").unwrap()).unwrap();
    ok(&mut e, &format!(r##"{{"op":"palette_set","index":{idx},"color":"#FF0000"}}"##));
    let p = ok(&mut e, r#"{"op":"get_pixel","layer":0,"x":0,"y":0}"#);
    assert_eq!(p.get("result").unwrap().get("color").unwrap().as_str(), Some("#FF0000"));
}
