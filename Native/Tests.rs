//! Pixel Studio native core tests (Rust built-in test harness only).
//!
//! One section per engine module; each imports the module it exercises.

mod animation {
    #[allow(unused_imports)]
    use crate::animation::*;

    #[test]
    fn orders() {
        assert_eq!(playback_order(0, 3, Direction::Forward), vec![0, 1, 2, 3]);
        assert_eq!(playback_order(0, 3, Direction::Reverse), vec![3, 2, 1, 0]);
        assert_eq!(playback_order(0, 3, Direction::PingPong), vec![0, 1, 2, 3, 2, 1]);
        assert_eq!(playback_order(2, 3, Direction::PingPong), vec![2, 3]);
        let t = timing(&[120, 80, 80, 150, 100]);
        assert_eq!(t.total_ms, 530);
        assert!((t.average_fps - 9.4339).abs() < 0.001);
    }
}

mod commands_json {
    #[allow(unused_imports)]
    use crate::commands::json::*;

    #[test]
    fn roundtrip() {
        let text = r#"{"op":"set_pixel","x":12,"y":-3,"f":1.5,"ok":true,"n":null,"s":"a\"b\\c\nd\u00e9\ud83d\ude00","a":[1,[2,{}],[]]}"#;
        let v = Json::parse(text).unwrap();
        assert_eq!(v.get("x").unwrap().as_i64(), Some(12));
        assert_eq!(v.get("y").unwrap().as_i64(), Some(-3));
        assert_eq!(v.get("s").unwrap().as_str(), Some("a\"b\\c\ndé😀"));
        let again = Json::parse(&v.to_json_string()).unwrap();
        assert_eq!(v, again);
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "{", "[1,]", "{\"a\" 1}", "01x", "\"abc", "tru", "[1] 2", "\"\\ud800\""] {
            assert!(Json::parse(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn depth_limit() {
        let deep = "[".repeat(1000) + &"]".repeat(1000);
        assert!(Json::parse(&deep).is_err());
    }
}

mod commands_voxel {
    #[allow(unused_imports)]
    use crate::commands::voxel::*;

    #[test]
    fn base64() {
        assert_eq!(base64_decode("SGVsbG8=").unwrap(), b"Hello");
        assert_eq!(base64_decode("data:image/png;base64,SGk=").unwrap(), b"Hi");
        assert!(base64_decode("!!").is_err());
    }
}

mod engine {
    use crate::commands::json::Json;
    #[allow(unused_imports)]
    use crate::engine::*;

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
        let info = ok(&mut e, r#"{"op":"canvas_inspect","detail":"animation","tag":"Walk"}"#);
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
        ok(&mut e, r#"{"op":"layer_set","layer":0,"name":"Body"}"#);
        ok(&mut e, r#"{"op":"layer_create","name":"Weapon"}"#);
        ok(&mut e, r##"{"op":"draw_line","layer":"Weapon","x0":10,"y0":2,"x1":10,"y1":12,"color":"#CCCCCC"}"##);
        ok(&mut e, r##"{"op":"fill_region","layer":"Body","x":0,"y":0,"color":"#884422"}"##);
        let r = ok(&mut e, r#"{"op":"read_region","layer":"Weapon","x":9,"y":1,"width":3,"height":3}"#);
        let rows = r.get("result").unwrap().get("rows").unwrap().as_array().unwrap();
        assert_eq!(rows[0].as_str(), Some("..."));
        assert_eq!(rows[1].as_str(), Some(".0."));
        ok(&mut e, r#"{"op":"layer_set","layer":"Weapon","locked":true}"#);
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
        let png = e.execute(br#"{"op":"export","format":"png","scale":4}"#, &[]);
        assert!(png.binary.starts_with(&[0x89, b'P', b'N', b'G']));
        let gif = e.execute(br#"{"op":"export","format":"gif"}"#, &[]);
        assert!(gif.binary.starts_with(b"GIF89a"));
        let sheet = e.execute(br#"{"op":"export","format":"spritesheet","columns":3}"#, &[]);
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
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let script = std::fs::read_to_string(root.join("Golden.jsonl")).expect("golden scene");
        let expected = std::fs::read_to_string(root.join("Golden.fingerprint")).expect("golden fingerprint");
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
        ok(
            &mut e,
            r##"{"op":"voxel_shape","shape":"ellipsoid","center":[24,57,24],"radius":5,"color":"#F0C090","bone":"head","jitter":0.3}"##,
        );
        ok(
            &mut e,
            r##"{"op":"voxel_shape","shape":"capsule","a":[28,29,24],"b":[28,2,24],"radius":3,"color":"#334455","color_to":"#556677","bone":"upper_leg_l","mirror_x":true}"##,
        );
        ok(&mut e, r#"{"op":"rig_bind","only_unbound":true}"#);
        let generated = ok(&mut e, r#"{"op":"anim3d_generate","kind":"walk","frames":8}"#);
        assert_eq!(generated.get("result").unwrap().get("frames").unwrap().as_i64(), Some(8));
        let m = &e.document().models[0];
        let v = m.get(19, 10, 24);
        assert_eq!(m.bones[v.bone as usize].name, "upper_leg_r", "mirror swaps _l/_r bones");

        let img =
            e.execute(br#"{"op":"voxel_render","yaw":45,"width":96,"animation":"walk","frame":2,"outline":true,"supersample":2}"#, &[]);
        assert!(img.json.contains("\"ok\":true"), "{}", img.json);
        let decoded = crate::export::png::decode(&img.binary).unwrap();
        assert!(decoded.pixels.chunks(4).filter(|p| p[3] > 0).count() > 200);
        let normal = e.execute(br#"{"op":"voxel_render","pass":"normal","width":64}"#, &[]);
        assert!(normal.json.contains("\"ok\":true"));
        let sheet = e.execute(br#"{"op":"voxel_render_sheet","animation":"walk","directions":8,"width":48,"pass":"albedo"}"#, &[]);
        let sheet_img = crate::export::png::decode(&sheet.binary).unwrap();
        assert_eq!((sheet_img.width, sheet_img.height), (48 * 8, 48 * 8));

        let baked =
            ok(&mut e, r#"{"op":"voxel_render_to_frames","animation":"walk","directions":8,"width":64,"height":64,"outline":true}"#);
        let r = baked.get("result").unwrap();
        assert_eq!(r.get("framesAdded").unwrap().as_i64(), Some(64));
        assert_eq!(r.get("tags").unwrap().as_array().unwrap().len(), 8);
        assert_eq!(e.document().frames.len(), 64, "the empty starting frame is reused");
        assert_eq!((e.document().width, e.document().height), (64, 64));
        let info = ok(&mut e, r#"{"op":"canvas_inspect","detail":"animation","tag":"walk_NE"}"#);
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
        let png = e.execute(br#"{"op":"export","format":"png"}"#, &[]).binary;
        ok(&mut e, r#"{"op":"layer_create","name":"Imported"}"#);
        let imp = e.execute(br#"{"op":"image_import","layer":"Imported","x":4,"y":4}"#, &png);
        assert!(imp.json.contains("\"ok\":true"), "{}", imp.json);
        let p = ok(&mut e, r#"{"op":"get_pixel","layer":"Imported","x":5,"y":4}"#);
        assert_eq!(p.get("result").unwrap().get("color").unwrap().as_str(), Some("#102031"), "a sparse native image keeps its grid");
        // Swatch edit recolors the art that uses the old color.
        ok(&mut e, r##"{"op":"palette_set","colors":["#102030"],"mode":"add"}"##);
        let idx = e.document().palette.find(crate::pixel::Rgba::parse_hex("#102030").unwrap()).unwrap();
        ok(&mut e, &format!(r##"{{"op":"palette_set","index":{idx},"color":"#FF0000"}}"##));
        let p = ok(&mut e, r#"{"op":"get_pixel","layer":0,"x":0,"y":0}"#);
        assert_eq!(p.get("result").unwrap().get("color").unwrap().as_str(), Some("#FF0000"));
    }

    #[test]
    fn game_production_pipeline() {
        let mut e = Engine::new(32, 32);
        // Blend modes composite differently from normal.
        ok(&mut e, r##"{"op":"draw_rectangle","x":0,"y":0,"width":32,"height":32,"color":"#806040","filled":true}"##);
        ok(&mut e, r#"{"op":"layer_create","name":"glow"}"#);
        ok(&mut e, r##"{"op":"draw_rectangle","layer":"glow","x":0,"y":0,"width":8,"height":8,"color":"#404040","filled":true}"##);
        let px = |e: &mut Engine| e.execute(br#"{"op":"render_frame"}"#, &[]).binary[(2 * 32 + 2) * 4..(2 * 32 + 2) * 4 + 3].to_vec();
        assert_eq!(px(&mut e), vec![0x40, 0x40, 0x40]);
        ok(&mut e, r#"{"op":"layer_set","layer":"glow","blend":"add"}"#);
        assert_eq!(px(&mut e), vec![0xC0, 0xA0, 0x80]);
        assert_eq!(err_code(&mut e, r#"{"op":"layer_set","blend":"sparkle"}"#), "invalid_argument");

        // Slices with pivots, surviving frame deletion.
        ok(&mut e, r#"{"op":"frame_add","count":2}"#);
        ok(&mut e, r#"{"op":"slice_set","name":"hitbox","x":4,"y":6,"width":10,"height":12,"pivot":[5,12]}"#);
        ok(&mut e, r#"{"op":"slice_set","name":"hitbox","frame":2,"x":6,"y":6,"width":12,"height":12}"#);
        ok(&mut e, r#"{"op":"frame_delete","frame":2}"#);
        let gd = ok(&mut e, r#"{"op":"canvas_inspect","detail":"gamedev"}"#).to_json_string();
        assert!(gd.contains("\"hitbox\"") && gd.contains("\"pivot\""), "{gd}");

        // Palette swap and RotSprite.
        ok(&mut e, r##"{"op":"palette_swap","map":{"#806040":"#2050A0"},"scope":"all"}"##);
        assert!(run(&mut e, r#"{"op":"get_pixel","layer":0,"frame":0,"x":20,"y":20}"#).to_json_string().contains("#2050A0"));
        ok(&mut e, r#"{"op":"layer_create","name":"sword"}"#);
        ok(&mut e, r##"{"op":"draw_line","layer":"sword","x0":16,"y0":4,"x1":16,"y1":27,"color":"#E0E0F0"}"##);
        let rot = ok(&mut e, r#"{"op":"rotate","layer":"sword","degrees":45}"#);
        assert!(rot.get("result").unwrap().get("changed").unwrap().as_i64().unwrap() > 10);

        // Blob tileset → terrain map → bake → Tiled export.
        let ts = ok(&mut e, r##"{"op":"tileset_generate_blob","name":"grass","tile_size":8,"fill":"#4A8C3F","lip":2}"##);
        assert_eq!(ts.get("result").unwrap().get("tileCount").unwrap().as_i64(), Some(47));
        ok(&mut e, r#"{"op":"tilemap_create","name":"level","tileset":"grass","columns":4,"rows":4}"#);
        ok(&mut e, r#"{"op":"tilemap_terrain","tilemap":"level","rows":["....",".##.",".###","...."]}"#);
        let tiled = e.execute(br#"{"op":"export","format":"tiled","tilemap":"level"}"#, &[]);
        assert!(tiled.binary.starts_with(&[0x89, b'P']));
        let tj = Json::parse(&tiled.json).unwrap();
        let tmj = tj.get("result").unwrap().get("tmj").unwrap();
        let data = tmj.get("layers").unwrap().as_array().unwrap()[0].get("data").unwrap().as_array().unwrap();
        assert_eq!(data.len(), 16);
        assert_eq!(data[0].as_i64(), Some(0));
        assert!(data[5].as_i64().unwrap() > 0);
        assert!(tiled.json.contains("wangsets"));
        ok(&mut e, r#"{"op":"layer_create","name":"ground"}"#);
        let baked = ok(&mut e, r#"{"op":"tilemap_bake","tilemap":"level","layer":"ground"}"#);
        assert!(baked.get("result").unwrap().get("changed").unwrap().as_i64().unwrap() > 50);
        assert_eq!(err_code(&mut e, r#"{"op":"tileset_delete","tileset":"grass"}"#), "conflict");

        // Plain tileset cut from the canvas, explicit tiles.
        ok(&mut e, r#"{"op":"tileset_create","name":"props","tile_size":8}"#);
        let cut = ok(&mut e, r#"{"op":"tileset_add_tiles","tileset":"props","x":0,"y":0,"columns":2,"rows":2}"#);
        assert!(cut.get("result").unwrap().get("tileCount").unwrap().as_i64().unwrap() >= 1);
        ok(&mut e, r#"{"op":"tilemap_create","name":"deco","tileset":"props","columns":3,"rows":2}"#);
        ok(&mut e, r#"{"op":"tilemap_set","tilemap":"deco","rows":["0 . 0", ". 0 ."]}"#);
        assert_eq!(err_code(&mut e, r#"{"op":"tilemap_set","tilemap":"deco","x":0,"y":0,"tile":99}"#), "not_found");

        // Lit preview, atlas and Godot export.
        let lit = e.execute(
            br##"{"op":"render_lit","lights":[{"x":4,"y":4,"color":"#FFB060","radius":40}],"bands":4,"unlit_layers":["sword"]}"##,
            &[],
        );
        assert!(lit.binary.starts_with(&[0x89, b'P']), "{}", lit.json);
        let atlas = e.execute(br#"{"op":"export","format":"atlas","padding":1,"extrude":1}"#, &[]);
        assert!(atlas.binary.starts_with(&[0x89, b'P']), "{}", atlas.json);
        let aj = Json::parse(&atlas.json).unwrap();
        let data = aj.get("result").unwrap().get("data").unwrap();
        assert_eq!(data.get("frames").unwrap().as_array().unwrap().len(), 2);
        assert!(data.get("meta").unwrap().get("slices").unwrap().as_array().unwrap().len() == 1);
        let godot = e.execute(br#"{"op":"export","format":"godot","texture_path":"res://hero.png"}"#, &[]);
        assert!(godot.json.contains("SpriteFrames") && godot.json.contains("AtlasTexture_1"), "{}", godot.json);

        // Everything survives a save/load round trip and undo.
        let saved = e.execute(br#"{"op":"project_save"}"#, &[]);
        let mut other = Engine::new(1, 1);
        let r = other.execute(br#"{"op":"project_load"}"#, &saved.binary);
        assert!(r.json.contains("\"ok\":true"), "{}", r.json);
        assert_eq!(other.document().tilemaps, e.document().tilemaps);
        assert_eq!(other.document().slices, e.document().slices);
        assert_eq!(other.document().layers, e.document().layers);
        ok(&mut e, r#"{"op":"history_undo"}"#);
        ok(&mut e, r#"{"op":"history_undo"}"#);
        assert_eq!(e.document().tilemaps.len(), 1);
    }

    #[test]
    fn asset_building_blocks_polygon_patterns_and_effects() {
        let mut e = Engine::new(64, 64);
        // A gable roof in one call, then shingles only where the roof is.
        let roof = ok(&mut e, r##"{"op":"draw_polygon","points":[[4,30],[32,8],[60,30]],"color":"#B9573D"}"##);
        assert!(roof.get("result").unwrap().get("changed").unwrap().as_i64().unwrap() > 500);
        let px = |e: &mut Engine, x: usize, y: usize| {
            let b = e.execute(br#"{"op":"render_frame"}"#, &[]).binary;
            [b[(y * 64 + x) * 4], b[(y * 64 + x) * 4 + 1], b[(y * 64 + x) * 4 + 2], b[(y * 64 + x) * 4 + 3]]
        };
        assert_eq!(px(&mut e, 32, 20)[3], 255, "inside the triangle");
        assert_eq!(px(&mut e, 4, 10)[3], 0, "outside stays empty");
        let tiles = ok(&mut e, r##"{"op":"pattern_fill","pattern":"shingle","size":4,"color":"#7A3329"}"##);
        assert!(tiles.get("result").unwrap().get("changed").unwrap().as_i64().unwrap() > 50);
        assert_eq!(px(&mut e, 4, 10)[3], 0, "patterns respect the shape (only_opaque)");
        for p in ["brick", "plank", "stone", "tile", "grass", "wave", "checker"] {
            let req = format!(
                r##"{{"op":"pattern_fill","pattern":"{p}","x":10,"y":20,"width":40,"height":10,"color":"#402020","color2":"#C08060"}}"##
            );
            ok(&mut e, &req);
        }
        assert_eq!(err_code(&mut e, r##"{"op":"pattern_fill","pattern":"lava","color":"#000000"}"##), "invalid_argument");
        assert_eq!(err_code(&mut e, r##"{"op":"draw_polygon","points":[[0,0],[5,5]],"color":"#000000"}"##), "invalid_argument");

        // Every effect kind in one call: frames are created, tagged and filled.
        let mut fx = Engine::new(48, 48);
        for (i, kind) in ["fire", "campfire", "slash", "hit", "magic", "smoke", "lightning", "heal", "explosion"].iter().enumerate() {
            let start = fx.document().frames.len() - if i == 0 { 1 } else { 0 };
            let req = format!(r#"{{"op":"fx_generate","kind":"{kind}","frame":{start},"tag":"{kind}"}}"#);
            let r = ok(&mut fx, &req);
            let frames = r.get("result").unwrap().get("frames").unwrap().as_i64().unwrap() as usize;
            assert!(r.get("result").unwrap().get("changed").unwrap().as_i64().unwrap() > 20, "{kind} draws pixels");
            assert_eq!(fx.document().frames.len(), start + frames);
        }
        assert_eq!(fx.document().tags.len(), 9);
        let custom = ok(
            &mut fx,
            r##"{"op":"fx_generate","kind":"explosion","frames":6,"size":24,"x":12,"y":12,"colors":["#FFFFFF","#C8F0FF","#5AB4FF"],"frame":0}"##,
        );
        assert_eq!(custom.get("result").unwrap().get("frames").unwrap().as_i64(), Some(6));
        assert_eq!(err_code(&mut fx, r#"{"op":"fx_generate","kind":"rainbow"}"#), "invalid_argument");
        let gif = fx.execute(br#"{"op":"export","format":"gif","tag":"fire"}"#, &[]);
        assert!(gif.binary.starts_with(b"GIF89a"));
    }

    #[test]
    fn advanced_vfx_particles_and_secondary_motion() {
        // Every effect kind: at least 8 frames, held peak timing for one-shots, glow pass.
        let kinds = [
            "fire",
            "campfire",
            "slash",
            "hit",
            "magic",
            "smoke",
            "lightning",
            "heal",
            "explosion",
            "shockwave",
            "beam",
            "portal",
            "ice",
            "poison",
            "splash",
            "dash",
            "shield",
            "levelup",
            "pickup",
            "dust",
            "holy",
            "shadow",
            "teleport",
        ];
        for kind in kinds {
            let mut e = Engine::new(64, 64);
            let r = ok(&mut e, &format!(r#"{{"op":"fx_generate","kind":"{kind}","glow":true}}"#));
            let res = r.get("result").unwrap();
            assert!(res.get("frames").unwrap().as_i64().unwrap() >= 8, "{kind} has at least 8 frames");
            assert!(res.get("changed").unwrap().as_i64().unwrap() > 40, "{kind} draws");
            assert!(e.document().layers.iter().any(|l| l.name == "Layer 1 Glow" && l.blend == crate::layers::Blend::Add));
            let d: Vec<i64> = res.get("durations").unwrap().as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
            if res.get("loop") == Some(&Json::Bool(false)) {
                assert!(d.iter().max() > d.iter().min(), "{kind}: one-shots hold the peak frame");
            } else {
                assert!(d.iter().all(|&x| x == d[0]), "{kind}: loops keep even timing");
            }
        }

        // Particles: a looping stream returns to its start; a burst falls under gravity.
        let mut e = Engine::new(64, 64);
        ok(&mut e, r##"{"op":"draw_rectangle","x":0,"y":56,"width":64,"height":8,"color":"#446644","filled":true}"##);
        ok(&mut e, r#"{"op":"layer_create","name":"Rain"}"#);
        ok(
            &mut e,
            r##"{"op":"fx_particles","layer":"Rain","frames":8,"emit":"stream","count":30,"origin":[32,0],"area":[30,0],"angle":95,"spread":5,"speed":[4,5],"gravity":0.1,"shape":"streak","colors":["#C9E6FF","#8FB8E8"],"tag":"rain"}"##,
        );
        assert_eq!(e.document().frames.len(), 8);
        let ground_kept = e.execute(br#"{"op":"render_frame","frame":5}"#, &[]).binary;
        assert_eq!(&ground_kept[(60 * 64 + 2) * 4..(60 * 64 + 2) * 4 + 3], &[0x44, 0x66, 0x44], "new frames hold the other layers");
        let burst = ok(
            &mut e,
            r##"{"op":"fx_particles","layer":"Rain","frames":8,"count":20,"origin":[32,20],"gravity":0.4,"shape":"star","clear":true}"##,
        );
        assert_eq!(burst.get("result").unwrap().get("loop"), Some(&Json::Bool(false)));
        assert_eq!(err_code(&mut e, r#"{"op":"fx_particles","shape":"heart"}"#), "invalid_argument");

        // Secondary motion: frame 0 untouched, others move, all layers stay aligned.
        let mut m = Engine::new(32, 32);
        ok(&mut m, r##"{"op":"draw_rectangle","x":12,"y":8,"width":8,"height":20,"color":"#7FD07A","filled":true}"##);
        ok(&mut m, r#"{"op":"layer_create","name":"Eyes"}"#);
        ok(&mut m, r##"{"op":"set_pixels","layer":"Eyes","pixels":[[14,12,"#000000"],[17,12,"#000000"]]}"##);
        let before = m.execute(br#"{"op":"render_frame","frame":0}"#, &[]).binary;
        for kind in ["breathe", "bob", "float", "sway", "squash", "shake", "pulse", "hurt"] {
            let r = ok(&mut m, &format!(r#"{{"op":"anim_generate","kind":"{kind}","frames":8}}"#));
            assert_eq!(r.get("result").unwrap().get("layers").unwrap().as_i64(), Some(2));
            assert_eq!(m.execute(br#"{"op":"render_frame","frame":0}"#, &[]).binary, before, "{kind}: frame 0 is the original pose");
            let moved = (1..8).any(|f| m.execute(format!(r#"{{"op":"render_frame","frame":{f}}}"#).as_bytes(), &[]).binary != before);
            assert!(moved, "{kind} animates");
        }
        assert_eq!(m.document().frames.len(), 8);
        let flash = m.execute(br#"{"op":"render_frame","frame":1}"#, &[]).binary;
        assert_eq!(&flash[(10 * 32 + 15) * 4..(10 * 32 + 15) * 4 + 3], &[255, 255, 255], "hurt starts with a white hit-flash");
        assert_eq!(err_code(&mut m, r#"{"op":"anim_generate","kind":"dance"}"#), "invalid_argument");
    }

    #[test]
    fn character_art_finishing_tools() {
        let mut e = Engine::new(48, 48);
        ok(&mut e, r##"{"op":"layer_set","layer":0,"name":"Face"}"##);
        ok(&mut e, r##"{"op":"draw_ellipse","layer":"Face","x":8,"y":10,"width":32,"height":30,"color":"#FCE3D2","filled":true}"##);
        ok(&mut e, r#"{"op":"layer_create","name":"Bangs"}"#);
        ok(&mut e, r##"{"op":"draw_rectangle","layer":"Bangs","x":8,"y":6,"width":32,"height":8,"color":"#F6A8C8","filled":true}"##);
        let before = e.execute(br#"{"op":"render_frame"}"#, &[]).binary;
        let r = ok(&mut e, r#"{"op":"auto_shade","layer":"Face","light":"top-left","tones":3,"cast_shadow":3}"#);
        assert!(r.get("result").unwrap().get("changed").unwrap().as_i64().unwrap() > 30);
        let after = e.execute(br#"{"op":"render_frame"}"#, &[]).binary;
        let lum =
            |b: &[u8], x: usize, y: usize| b[(y * 48 + x) * 4] as u32 + b[(y * 48 + x) * 4 + 1] as u32 + b[(y * 48 + x) * 4 + 2] as u32;
        assert!(lum(&after, 24, 15) < lum(&before, 24, 15), "bangs cast a shadow on the forehead");
        ok(&mut e, r##"{"op":"auto_shade","layer":"Bangs","ramps":{"#F6A8C8":["#D97BA8","#FFD3E6"]},"cast_shadow":0}"##);
        ok(&mut e, r#"{"op":"outline","layer":"Bangs","mode":"selout","darkness":0.45}"#);
        assert_eq!(err_code(&mut e, r#"{"op":"outline","mode":"glow"}"#), "invalid_argument");
        ok(&mut e, r#"{"op":"layer_create","name":"FX"}"#);
        let sp = ok(&mut e, r##"{"op":"draw_sparkle","layer":"FX","points":[[4,4,3],[42,8],[40,40,5]],"color":"#FFD6A0"}"##);
        assert!(sp.get("result").unwrap().get("changed").unwrap().as_i64().unwrap() > 20);
        ok(&mut e, r##"{"op":"gradient_map","scope":"frame","colors":["#1B1F4A","#5A7BC0","#D8ECFF"],"strength":0.6}"##);
        ok(&mut e, r#"{"op":"pixel_cleanup","layer":"Bangs"}"#);
        ok(&mut e, r#"{"op":"history_undo","steps":3}"#);
    }

    fn px(b: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
        let o = (y * w + x) * 4;
        [b[o], b[o + 1], b[o + 2], b[o + 3]]
    }

    #[test]
    fn material_ramps_shade_brush_and_palette_lock() {
        let mut e = Engine::new(32, 32);
        ok(&mut e, r##"{"op":"ramp_set","name":"skin","colors":["#583C2F","#89563E","#BF814F","#F5C6A2"]}"##);
        ok(&mut e, r##"{"op":"color_ramp","base":"#579E83","steps":4,"name":"cloak"}"##);
        assert_eq!(err_code(&mut e, r##"{"op":"ramp_set","name":"x","colors":["#000000"]}"##), "invalid_argument");
        ok(&mut e, r##"{"op":"draw_rectangle","x":4,"y":4,"width":20,"height":20,"color":"#BF814F","filled":true}"##);
        // Shade brush: one step darker inside the ramp, clamped at its ends.
        let r = ok(&mut e, r#"{"op":"shade","x":4,"y":4,"width":3,"height":1,"steps":-1}"#);
        assert_eq!(r.get("result").unwrap().get("changed").unwrap().as_i64(), Some(3));
        ok(&mut e, r#"{"op":"shade","pixels":[[4,4]],"steps":-9}"#);
        let img = e.execute(br#"{"op":"render_frame"}"#, &[]).binary;
        assert_eq!(px(&img, 32, 5, 4), [0x89, 0x56, 0x3E, 255]);
        assert_eq!(px(&img, 32, 4, 4), [0x58, 0x3C, 0x2F, 255]);
        // Palette-locked auto shading only uses ramp colors.
        ok(&mut e, r##"{"op":"draw_rectangle","x":26,"y":26,"width":5,"height":5,"color":"#123456","filled":true}"##);
        ok(&mut e, r#"{"op":"auto_shade","light":"top-left","tones":4,"cast_shadow":0}"#);
        let img = e.execute(br#"{"op":"render_frame"}"#, &[]).binary;
        let allowed = [[0x58, 0x3C, 0x2F], [0x89, 0x56, 0x3E], [0xBF, 0x81, 0x4F], [0xF5, 0xC6, 0xA2], [0x12, 0x34, 0x56]];
        let mut seen = std::collections::HashSet::new();
        for y in 0..32 {
            for x in 0..32 {
                let p = px(&img, 32, x, y);
                if p[3] > 0 {
                    assert!(allowed.contains(&[p[0], p[1], p[2]]), "invented color {p:?}");
                    seen.insert(p);
                }
            }
        }
        assert!(seen.len() >= 4, "the ramp is actually used");
        // Ramps survive save/load.
        let saved = e.execute(br#"{"op":"project_save"}"#, &[]);
        let mut other = Engine::new(4, 4);
        assert!(other.execute(br#"{"op":"project_load"}"#, &saved.binary).json.contains("\"ok\":true"));
        let r = ok(&mut other, r#"{"op":"shade","x":5,"y":4,"steps":1}"#);
        assert_eq!(r.get("result").unwrap().get("changed").unwrap().as_i64(), Some(1));
        ok(&mut e, r#"{"op":"ramp_delete","name":"cloak"}"#);
        assert_eq!(err_code(&mut e, r#"{"op":"ramp_delete","name":"cloak"}"#), "not_found");
    }

    #[test]
    fn mixed_pixel_density_layers() {
        let mut e = Engine::new(16, 16);
        ok(&mut e, r##"{"op":"draw_rectangle","x":0,"y":0,"width":16,"height":16,"color":"#202040","filled":true}"##);
        ok(&mut e, r#"{"op":"layer_create","name":"Eyes"}"#);
        let r = ok(&mut e, r#"{"op":"layer_set","layer":"Eyes","density":4,"opacity":255}"#);
        assert_eq!(r.get("result").unwrap().get("density").unwrap().as_i64(), Some(4));
        // In a 4× layer, coordinates address quarter pixels: 64×64 grid.
        ok(&mut e, r##"{"op":"set_pixel","layer":"Eyes","x":63,"y":63,"color":"#FFFFFF"}"##);
        ok(&mut e, r##"{"op":"draw_rectangle","layer":"Eyes","x":60,"y":60,"width":4,"height":3,"color":"#FFFFFF","filled":true}"##);
        ok(&mut e, r##"{"op":"set_pixel","layer":"Eyes","x":1,"y":2,"color":"#FF0000"}"##);
        let img = e.execute(br#"{"op":"render_frame"}"#, &[]).binary;
        assert_eq!(img.len(), 64 * 64 * 4, "render follows the finest layer");
        assert_eq!(px(&img, 64, 63, 63), [255, 255, 255, 255]);
        assert_eq!(px(&img, 64, 62, 63), [0x20, 0x20, 0x40, 255]);
        assert_eq!(px(&img, 64, 59, 60), [0x20, 0x20, 0x40, 255]);
        assert_eq!(px(&img, 64, 1, 2), [255, 0, 0, 255]);
        let small = e.execute(br#"{"op":"render_frame","density":1}"#, &[]).binary;
        assert_eq!(small.len(), 16 * 16 * 4);
        // Reads: a layer read uses its fine grid, a composite read uses canvas pixels.
        let fine = ok(&mut e, r#"{"op":"get_pixel","layer":"Eyes","x":1,"y":2}"#);
        assert_eq!(fine.get("result").unwrap().get("color").unwrap().as_str(), Some("#FF0000"));
        let comp = ok(&mut e, r#"{"op":"get_pixel","composite":true,"x":15,"y":15}"#);
        assert_eq!(comp.get("result").unwrap().get("color").unwrap().as_str(), Some("#FFFFFF"), "the 4x4 white block wins the vote");
        assert_eq!(err_code(&mut e, r#"{"op":"get_pixel","composite":true,"x":40,"y":2}"#), "invalid_argument");
        // Density survives save/load; lowering it resamples the cel.
        let saved = e.execute(br#"{"op":"project_save"}"#, &[]);
        let mut other = Engine::new(4, 4);
        assert!(other.execute(br#"{"op":"project_load"}"#, &saved.binary).json.contains("\"ok\":true"));
        assert_eq!(other.execute(br#"{"op":"render_frame"}"#, &[]).binary, img);
        ok(&mut other, r#"{"op":"layer_set","layer":"Eyes","density":1,"name":"Detail","locked":true}"#);
        let img = other.execute(br#"{"op":"render_frame"}"#, &[]).binary;
        assert_eq!(img.len(), 16 * 16 * 4);
        assert_eq!(px(&img, 16, 15, 15), [255, 255, 255, 255]);
        assert_eq!(err_code(&mut other, r#"{"op":"layer_set","layer":"Detail","density":3}"#), "invalid_argument");
    }

    #[test]
    fn pixel_perfect_strokes() {
        let mut e = Engine::new(16, 16);
        let r = ok(&mut e, r##"{"op":"draw_stroke","points":[[0,0],[1,0],[1,1],[2,1],[2,2],[3,2]],"color":"#000000"}"##);
        // The staircase loses its L-corner doubles: 4 pixels instead of 6.
        assert_eq!(r.get("result").unwrap().get("changed").unwrap().as_i64(), Some(4));
        let raw = ok(&mut e, r##"{"op":"draw_stroke","points":[[0,8],[1,8],[1,9],[2,9]],"color":"#000000","pixel_perfect":false}"##);
        assert_eq!(raw.get("result").unwrap().get("changed").unwrap().as_i64(), Some(4));
        let thick = ok(&mut e, r##"{"op":"draw_stroke","points":[[4,12],[12,12]],"color":"#FF0000","size":2}"##);
        assert_eq!(thick.get("result").unwrap().get("changed").unwrap().as_i64(), Some(20));
    }

    #[test]
    fn reference_import_recovers_non_integer_grid() {
        // A 20×14 native image with a frame color, upscaled 3.35× by nearest neighbour.
        let (nw, nh) = (20u32, 14u32);
        let mut native = crate::renderer::Image::new(nw, nh);
        let mut rng = 7u32;
        for y in 0..nh {
            for x in 0..nw {
                rng = rng.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let edge = x == 0 || y == 0 || x == nw - 1 || y == nh - 1;
                let c: [u8; 4] = if edge {
                    [255, 255, 255, 255]
                } else {
                    [
                        [0x58, 0x3C, 0x2F, 255],
                        [0x89, 0x56, 0x3E, 255],
                        [0xBF, 0x81, 0x4F, 255],
                        [0x2A, 0x5B, 0x49, 255],
                        [0x57, 0x9E, 0x83, 255],
                    ][(rng >> 16) as usize % 5]
                };
                let o = ((y * nw + x) * 4) as usize;
                native.pixels[o..o + 4].copy_from_slice(&c);
            }
        }
        let (bw, bh) = (67u32, 47u32);
        let mut big = crate::renderer::Image::new(bw, bh);
        for y in 0..bh {
            for x in 0..bw {
                let c = native.get(x * nw / bw, y * nh / bh);
                let o = ((y * bw + x) * 4) as usize;
                big.pixels[o..o + 4].copy_from_slice(&[c.r, c.g, c.b, c.a]);
            }
        }
        let png = crate::export::png::encode(&big);
        let mut e = Engine::new(32, 32);
        let r = e.execute(br#"{"op":"image_import","grid":"auto","ramps":true,"x":2,"y":3}"#, &png);
        let j = Json::parse(&r.json).unwrap();
        assert_eq!(j.get("ok"), Some(&Json::Bool(true)), "{}", r.json);
        let res = j.get("result").unwrap();
        assert_eq!(res.get("native").unwrap().get("width").unwrap().as_i64(), Some(20));
        assert_eq!(res.get("native").unwrap().get("height").unwrap().as_i64(), Some(14));
        assert!(!res.get("ramps").unwrap().as_array().unwrap().is_empty());
        let img = e.execute(br#"{"op":"render_frame"}"#, &[]).binary;
        assert_eq!(px(&img, 32, 2, 3)[3], 0, "the white frame became transparent");
        for (x, y) in [(5u32, 5u32), (10, 7), (18, 12)] {
            let c = native.get(x, y);
            assert_eq!(px(&img, 32, (x + 2) as usize, (y + 3) as usize), [c.r, c.g, c.b, c.a], "pixel {x},{y}");
        }
        // The extracted ramps drive the shade brush right away.
        let r = ok(&mut e, r#"{"op":"shade","x":3,"y":4,"width":18,"height":12,"steps":1}"#);
        assert!(r.get("result").unwrap().get("changed").unwrap().as_i64().unwrap() > 20);
        // As a reference it lands on a locked, faded layer.
        let r = e.execute(br#"{"op":"image_import","reference":true}"#, &png);
        assert!(r.json.contains("\"layer\":\"Reference\""), "{}", r.json);
    }
}

mod artkit_tools {
    use crate::commands::json::Json;
    use crate::engine::Engine;

    fn run(e: &mut Engine, req: &str) -> Json {
        Json::parse(&e.execute(req.as_bytes(), &[]).json).unwrap()
    }

    fn ok(e: &mut Engine, req: &str) -> Json {
        let j = run(e, req);
        assert_eq!(j.get("ok"), Some(&Json::Bool(true)), "request {req} failed: {}", j.to_json_string());
        j.get("result").unwrap().clone()
    }

    fn num(j: &Json, k: &str) -> f64 {
        j.get(k).and_then(Json::as_f64).unwrap_or_else(|| panic!("no number '{k}' in {}", j.to_json_string()))
    }

    fn codes(review: &Json) -> Vec<String> {
        review.get("issues").unwrap().as_array().unwrap().iter().map(|i| i.get("code").unwrap().as_str().unwrap().to_string()).collect()
    }

    const BALL: &str = r##"{"op":"ramp_set","name":"red","colors":["#3A0E14","#6E1A22","#A8303A","#D8624E","#F5A27C"]}"##;

    #[test]
    fn critic_tells_pillow_shading_from_form_lighting() {
        // Pillow: concentric rings that follow the outline.
        let mut e = Engine::new(40, 40);
        ok(&mut e, BALL);
        for (i, c) in ["#3A0E14", "#6E1A22", "#A8303A", "#D8624E", "#F5A27C"].iter().enumerate() {
            let r = 16 - i as i64 * 3;
            ok(
                &mut e,
                &format!(
                    r#"{{"op":"draw_ellipse","x":{},"y":{},"width":{},"height":{},"color":"{c}","filled":true}}"#,
                    20 - r,
                    20 - r,
                    2 * r,
                    2 * r
                ),
            );
        }
        let pillow = ok(&mut e, r#"{"op":"art_review"}"#);
        assert!(codes(&pillow).contains(&"pillow_shading".to_string()), "{}", pillow.to_json_string());
        // Same ball, lit by its surface normals from the top-left.
        let mut e = Engine::new(40, 40);
        ok(&mut e, BALL);
        ok(&mut e, r##"{"op":"draw_ellipse","x":4,"y":4,"width":32,"height":32,"color":"#A8303A","filled":true}"##);
        let lit = ok(&mut e, r#"{"op":"shade_form","form":"sphere","cx":20,"cy":20,"rx":16,"shadow_steps":2,"light_steps":2}"#);
        assert!(num(&lit, "changed") > 300.0);
        let review = ok(&mut e, r#"{"op":"art_review"}"#);
        assert!(!codes(&review).contains(&"pillow_shading".to_string()), "{}", review.to_json_string());
        // The lit side really is up-left: compare two pixels.
        let px = |e: &mut Engine, x: i32, y: i32| {
            let r = ok(e, &format!(r#"{{"op":"get_pixel","x":{x},"y":{y}}}"#));
            r.get("color").unwrap().as_str().unwrap().to_string()
        };
        let (light, dark) = (px(&mut e, 12, 12), px(&mut e, 30, 30));
        assert_ne!(light, dark);
        assert!(["#D8624E", "#F5A27C"].contains(&light.as_str()), "{light}");
        assert!(["#3A0E14", "#6E1A22"].contains(&dark.as_str()), "{dark}");
        // Inflate works on any silhouette and only uses ramp colours.
        ok(&mut e, r##"{"op":"draw_polygon","points":[[2,2],[14,3],[10,14],[3,12]],"color":"#A8303A"}"##);
        ok(&mut e, r#"{"op":"shade_form","x":0,"y":0,"width":16,"height":16}"#);
    }

    #[test]
    fn critic_measures_values_banding_and_returns_a_marked_png() {
        let mut e = Engine::new(32, 32);
        ok(&mut e, r##"{"op":"draw_rectangle","x":0,"y":0,"width":32,"height":32,"color":"#202830","filled":true}"##);
        // Flat, muddy figure with stacked equal runs (banding).
        for (i, c) in ["#3A3F4A", "#454A55", "#50555F", "#3A3F4A"].iter().enumerate() {
            ok(&mut e, &format!(r#"{{"op":"draw_rectangle","x":8,"y":{},"width":10,"height":1,"color":"{c}","filled":true}}"#, 8 + i));
        }
        ok(&mut e, r##"{"op":"draw_rectangle","x":8,"y":12,"width":10,"height":10,"color":"#454A55","filled":true}"##);
        let r = e.execute(br#"{"op":"art_review","scale":2}"#, &[]);
        let j = Json::parse(&r.json).unwrap();
        let res = j.get("result").unwrap();
        assert!(num(res.get("banding").unwrap(), "pairs") >= 3.0, "{}", r.json);
        let c = codes(res);
        assert!(c.contains(&"low_value_contrast".to_string()) && c.contains(&"figure_blends_into_background".to_string()), "{c:?}");
        assert_eq!(&r.binary[1..4], b"PNG");
        // fix_banding lowers the error.
        let fixed = ok(&mut e, r#"{"op":"fix_banding"}"#);
        assert!(num(&fixed, "after") < num(&fixed, "before"), "{}", fixed.to_json_string());
    }

    #[test]
    fn curves_are_smooth_tapered_and_fillable() {
        let mut e = Engine::new(64, 64);
        let thin = ok(&mut e, r##"{"op":"draw_path","points":[[4,40],[16,20],[34,12],[56,16]],"color":"#000000"}"##);
        assert!(num(&thin, "changed") > 40.0);
        // The 1-px curve is pixel-perfect: no pixel has 3+ path neighbours in an L.
        let review = ok(&mut e, r#"{"op":"art_review"}"#);
        assert_eq!(num(&review, "jaggies"), 0.0, "{}", review.to_json_string());
        let mut t = Engine::new(64, 64);
        let taper = ok(&mut t, r##"{"op":"draw_path","points":[[6,56],[20,30],[40,12]],"width":[6,1],"color":"#000000"}"##);
        assert!(num(&taper, "changed") > num(&thin, "changed"));
        let mut f = Engine::new(64, 64);
        let blob =
            ok(&mut f, r##"{"op":"draw_path","points":[[32,8],[54,30],[32,56],[10,30]],"closed":true,"filled":true,"color":"#336699"}"##);
        assert!(num(&blob, "changed") > 900.0, "{}", blob.to_json_string());
        let mut b = Engine::new(64, 64);
        ok(&mut b, r##"{"op":"draw_path","curve":"bezier","points":[[4,60],[4,4],[60,4],[60,60]],"color":"#000000"}"##);
        let bad = run(&mut b, r##"{"op":"draw_path","curve":"bezier","points":[[4,60],[4,4],[60,4]],"color":"#000000"}"##);
        assert_eq!(bad.get("ok"), Some(&Json::Bool(false)));
    }

    #[test]
    fn monotone_runs_remove_jaggies() {
        // Runs 3,1,4,1,3 on a gentle curve become monotone, endpoints kept.
        let mut path = vec![(0i64, 0i64)];
        let mut p = (0i64, 0i64);
        for (k, run) in [3, 3, 1, 4, 1, 3, 4, 4].iter().enumerate() {
            for s in 0..*run {
                if k == 0 && s == 0 {
                    continue;
                }
                p = if s == 0 { (p.0 + 1, p.1 + 1) } else { (p.0 + 1, p.1) };
                path.push(p);
            }
        }
        let (out, fixed) = crate::commands::artkit::smooth_runs(&path);
        assert_eq!(fixed, 1);
        assert_eq!(out.first(), path.first());
        assert_eq!(out.last(), path.last());
        let mut runs = vec![1];
        for w in out.windows(2) {
            if w[1].1 == w[0].1 { *runs.last_mut().unwrap() += 1 } else { runs.push(1) }
        }
        let inner = &runs[1..runs.len() - 1];
        assert!(inner.windows(2).all(|w| w[1] >= w[0]), "{runs:?}");
    }

    #[test]
    fn selective_antialias_softens_steps_inside_only() {
        let mut e = Engine::new(32, 16);
        ok(&mut e, r##"{"op":"ramp_set","name":"blue","colors":["#101830","#283C70","#4A6AB0","#86A8E0"]}"##);
        ok(&mut e, r##"{"op":"draw_rectangle","x":0,"y":0,"width":32,"height":16,"color":"#86A8E0","filled":true}"##);
        // A shallow staircase (runs of 4) in the darkest tone.
        for k in 0..6 {
            ok(
                &mut e,
                &format!(r##"{{"op":"draw_rectangle","x":{},"y":{},"width":4,"height":1,"color":"#101830","filled":true}}"##, k * 4, 2 + k),
            );
        }
        let aa = ok(&mut e, r#"{"op":"antialias","amount":0.5}"#);
        assert!(num(&aa, "changed") >= 8.0, "{}", aa.to_json_string());
        let mid = ok(&mut e, r#"{"op":"get_pixel","x":4,"y":2}"#);
        assert_eq!(mid.get("color").unwrap().as_str(), Some("#4A6AB0"), "the in-between ramp tone");
        // 45° lines and transparent edges are left alone.
        let mut d = Engine::new(16, 16);
        ok(&mut d, r##"{"op":"draw_rectangle","x":0,"y":0,"width":16,"height":16,"color":"#FFFFFF","filled":true}"##);
        ok(&mut d, r##"{"op":"draw_line","x0":0,"y0":0,"x1":15,"y1":15,"color":"#000000"}"##);
        assert_eq!(num(&ok(&mut d, r#"{"op":"antialias","allow_new_colors":true}"#), "changed"), 0.0);
        let mut t = Engine::new(16, 8);
        ok(&mut t, r##"{"op":"draw_rectangle","x":0,"y":3,"width":5,"height":1,"color":"#000000","filled":true}"##);
        ok(&mut t, r##"{"op":"draw_rectangle","x":5,"y":4,"width":5,"height":1,"color":"#000000","filled":true}"##);
        assert_eq!(num(&ok(&mut t, r#"{"op":"antialias","allow_new_colors":true}"#), "changed"), 0.0);
    }

    const JPEG_QUADS: &str = "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAIBAQEBAQIBAQECAgICAgQDAgICAgUEBAMEBgUGBgYFBgYGBwkIBgcJBwYGCAsICQoKCgoKBggLDAsKDAkKCgr/2wBDAQICAgICAgUDAwUKBwYHCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgr/wAARCAAQABADASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwD53r3iviev6KK/M/pJ8Gf8QE/sr9/9c+ue3+z7Hk9j7H+9V5ub2v8Adty9b6fQeMGc/wDEz/1L93/Z/wDZ/tevt/ae39n5UeTk9j/e5ubpbX//2Q==";
    const JPEG_CONCEPT: &str = "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAUDBAQEAwUEBAQFBQUGBwwIBwcHBw8LCwkMEQ8SEhEPERETFhwXExQaFRERGCEYGh0dHx8fExciJCIeJBweHx7/2wBDAQUFBQcGBw4ICA4eFBEUHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh7/wAARCABJAFkDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwDmqXigUdDzX9Cn9Uidu1LijrRGjTOIYUMsshCIicszHgAAdSTXJjcbSwlJ1JySsm0m7Xt2M6lRU4ttgMe1HarX/CM+Ix/zL+rD/tzk/wAKB4a8R5x/wj2rAn/pzk/wr8q/4iu/+gP/AMn/APtDw/7fp9l/4F/wCqeuMCgA5xVo+GvEYGP+Ef1X2/0OT/CoLiyvNOcQ6haT2crDeI5oyjFfXBHTIPNe1kHiDDNsYsNUo+zVm+Zzvt6xX5nThc3p4ipyaL5jMc460EUdRkHIPvR3zX6LGSklKLumesndXA+tJSn/ADzSVRSFo79qB6YoHPNAmHTFW/BmD4u0Q46ahB/6MWqgz+FXvBsJXxdozEji/g5B/wCmi1+XeJOVYzHrDTw1NyUOe9unw/5M8TO6M6lK8Veyf5H0J4o13SfDOh3Gt65d/ZNOttvnSiNn2hmCrwoLfeYDpWF4L+JPgnxjqculeHNa+3XccBuHj+yzR4jDKpOXUDqw4znmk+MfhjUPGPw51Xw3pstvDd3nleW9yxWMBZkc5IBPRT2POK86/Z++D3ib4f8AjS71nWr7SLi3m097VFtJpGfc0kbAkNGoxhD364r8qpUqLoSlKXvLZH4dOdRVEorTqeoeOPHfhTwStoPE2qiwN7v+z/uJZN+zbu/1anGN69eua8b+I3i7w94y1mHU/Dd+L6zitlt3k8mSPEgZ2Iw6gnhl5AxzXWftG/DLX/iK2hDRbvTbf+z/ALR532yR13eZ5e3btRv7hznFeWWngHWPAEZ0bWbqwuLiYm5V7R3ZArfKAdyqc5Q9vStqWVyx1JQwycqr+yuyPc4dVepmXs4xurP8izDxEO/XrT/1FNjXamD+lO96/ozI6M6GWYelUVpRhBNdmopNH7VQi40op7pIO3IpKU0leoboP5Uo6Z5o7UY69vrQJgfWmaPrY0/VrO/+zeb9nuEl2eZjdtYHGccdOtVZruVJWUKhAJA61TPfP51+VcUcYOfLRwFRr4lO6WuyW6fnsfhPGviNNyhRyiq4251O8VrslbmT/vbWPXv+F2H/AKFn6f6d/wDa6D8bDkN/wjWOe9//APa68elfbEWGDtIwKrLcts4C5r8tcKa6H57g8RnGMpudKasnbaP+R7V/wuv08NHJ/wCn7/7XXGeOfGX/AAk+rw3/APZotNluIdnn784ZjnO0f3q46CUyMwbHGO3rUvQgnnjp6V6GXY2tl1b2+Flyy2vZPf1uhQ4jznKcS+SryzXlF7rzRrwP5kSvgjOeM0+syG7kjTYFBA6ZHNaMTFo1YgZKg1+5cO8QUc0pRppt1IxXNpbXr5bn9CcH8W4bPaEaSk3WhCLndWXNZJ26b32VhxFJTvbNNr6VH2yF7e1GB+FJ2pe+etAmQm1hYlinLHJ5NZnU4PQc5ra7kVWvI4xayssaKRGxB2jrivhOKOFaeNhGrhlGnyKTdlbm2fT0e/c/MONOAaWaxhVwShRcOZytFLmvZq9kr2s9+5luoYbDkjpjNN8iPPK8Z9TSaWS+oRhiWU5yDyOhrb8qHj90n/fIr4PIeFKudYd4iE1FJ8tmn2T/AFPgMh8Pcxx2HlUoYpQSdre9vZa6epjxqsfKjBx3zSjjBzz6CpddAjEGwBM7s7RjPSpNERZLRmkAc7yAWGT0FZ0+Gas82llamuaKvfW2yf6nO/DrGV80lgp4hOSV+Zpvon69Se0t43t0Zlyee5q2qqqhV6AYHNIFCgBQAB0A4pQeMYr9myrKqGX0IQhCKkopNpJNtLd9Xd66n71kWRYXKMLTp06cVUUYxlKMUnJpK7btd3euocUlLSV6qPdQoopKKAsKTmqPiCN5tB1GKNC7vayqqqMkkocADuau0VnVp+0g4PqrGdaiqtOVN9U1955d4E0vU7bxVZy3GnXkMS79zyQMqj92wHJGPavUu/NJRXnZPlMMroOjCTabvr6JfoeRkOR08mw0qFObknJu780l+hxPxTsr28/s02dpcXPl+bu8qMttzsxnA+tXfhla3VpoM8d5bzQSG6ZgssZUkbV55+ldTRWcMmhHMZY/md2rW6bJfoZ0+H6cM2lmam+aStbpsl+gv5dKKSivZse/YXpSUUUDSsf/2Q==";

    #[test]
    fn baseline_jpeg_decodes() {
        let bytes = crate::commands::voxel::base64_decode(JPEG_QUADS).unwrap();
        let img = crate::export::decode_image(&bytes).unwrap();
        assert_eq!((img.width, img.height), (16, 16));
        let close = |x: u32, y: u32, c: [i32; 3]| {
            let p = img.get(x, y);
            assert!(
                (p.r as i32 - c[0]).abs() < 24 && (p.g as i32 - c[1]).abs() < 24 && (p.b as i32 - c[2]).abs() < 24,
                "({x},{y}) = {p:?} vs {c:?}"
            );
        };
        close(3, 3, [220, 40, 40]);
        close(12, 3, [40, 180, 60]);
        close(3, 12, [40, 60, 200]);
        close(12, 12, [240, 220, 60]);
        assert!(crate::export::decode_image(b"\xFF\xD8\xFF\xC2junk").is_err());
    }

    #[test]
    fn ai_concept_becomes_true_pixel_art() {
        // 12×10 sprite on magenta, upscaled 7× with noise and saved as a 4:2:0 JPEG (q85).
        let mut e = Engine::new(64, 64);
        let r = ok(&mut e, &format!(r#"{{"op":"concept_import","data_base64":"{JPEG_CONCEPT}","colors":12,"margin":0}}"#));
        assert!((num(&r, "cell") - 7.0).abs() < 0.15, "{}", r.to_json_string());
        let sprite = r.get("sprite").unwrap();
        assert_eq!((num(sprite, "width"), num(sprite, "height")), (8.0, 9.0), "{}", r.to_json_string());
        assert!(num(&r, "backgroundPixels") > 40.0);
        // The canvas fits the sprite; the eye pixels came through dark, the skin light.
        let insp = ok(&mut e, r#"{"op":"canvas_inspect"}"#);
        assert_eq!((num(&insp, "width"), num(&insp, "height")), (8.0, 9.0));
        let px = |e: &mut Engine, x: i32, y: i32| {
            let c = ok(e, &format!(r#"{{"op":"get_pixel","layer":"Concept","x":{x},"y":{y}}}"#));
            c.get("color").and_then(Json::as_str).map(|s| s.to_string())
        };
        // The crop starts at the body row's left edge (sprite column 2): eye at x 3, skin at x 2.
        let eye = px(&mut e, 3, 2).unwrap();
        let skin = px(&mut e, 2, 2).unwrap();
        let lum = |h: &str| {
            u32::from_str_radix(&h[1..3], 16).unwrap()
                + u32::from_str_radix(&h[3..5], 16).unwrap()
                + u32::from_str_radix(&h[5..7], 16).unwrap()
        };
        assert!(lum(&eye) < 150 && lum(&skin) > 450, "eye {eye} skin {skin}");
        assert_eq!(px(&mut e, 0, 0), None, "magenta background keyed out");
    }

    #[test]
    fn mannequin_is_posed_lit_and_reports_joints() {
        let mut e = Engine::new(128, 128);
        let warn = ok(&mut e, r##"{"op":"ramp_set","name":"flat","colors":["#606060","#686868","#707070"]}"##);
        assert_eq!(warn.get("warnings").unwrap().as_array().unwrap().len(), 1, "narrow ramps are flagged");
        assert!(run(&mut e, r#"{"op":"draw_figure","x":64,"y":120}"#).get("ok") == Some(&Json::Bool(false)), "needs a skin ramp");
        ok(&mut e, r##"{"op":"ramp_set","name":"skin","colors":["#5A2F2A","#8A4B3A","#B8714F","#DDA078","#F4CDA6","#FFE9CF"]}"##);
        for pose in ["contrapposto", "walk", "stand"] {
            let r = ok(&mut e, &format!(r#"{{"op":"draw_figure","x":64,"y":120,"height":100,"pose":"{pose}","name":"{pose}"}}"#));
            let j = r.get("joints").unwrap();
            let head = j.get("head").unwrap().as_array().unwrap()[1].as_f64().unwrap();
            let ankle = j.get("ankleNear").unwrap().as_array().unwrap()[1].as_f64().unwrap();
            assert!(head < 35.0 && ankle > 110.0, "{}", r.to_json_string());
            assert!(num(&r, "changed") > 600.0);
        }
        let skin = ["#5A2F2A", "#8A4B3A", "#B8714F", "#DDA078", "#F4CDA6", "#FFE9CF"];
        let img = e.execute(br#"{"op":"render_frame","layer":"walk"}"#, &[]).binary;
        let mut seen = std::collections::HashSet::new();
        for p in img.chunks(4).filter(|p| p[3] > 0) {
            let hex = format!("#{:02X}{:02X}{:02X}", p[0], p[1], p[2]);
            assert!(skin.contains(&hex.as_str()), "invented colour {hex}");
            seen.insert(hex);
        }
        assert!(seen.len() >= 3, "form lighting uses several tones: {seen:?}");
    }
}

mod export_deflate {
    #[allow(unused_imports)]
    use crate::export::deflate::*;

    #[test]
    fn known_checksums() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn deflate_roundtrip() {
        let mut samples: Vec<Vec<u8>> = vec![vec![], b"a".to_vec(), b"abcabcabcabcabcabc".to_vec(), vec![7u8; 100_000]];
        let mut rng = 12345u32;
        let mut noise = Vec::new();
        for _ in 0..70_000 {
            rng = rng.wrapping_mul(1_103_515_245).wrapping_add(12345);
            noise.push((rng >> 16) as u8 % 7);
        }
        samples.push(noise);
        for s in samples {
            let z = zlib_compress(&s);
            assert_eq!(crate::export::inflate::zlib_decompress(&z, 1 << 24).unwrap(), s);
        }
        assert!(zlib_compress(&[7u8; 100_000]).len() < 2_000);
    }
}

mod export_gif {
    #[allow(unused_imports)]
    use crate::export::gif::*;
    use crate::renderer::Image;

    #[test]
    fn lzw_roundtrip_including_table_reset() {
        let mut rng = 99u32;
        let mut data = Vec::new();
        for _ in 0..200_000 {
            rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            data.push((rng >> 24) as u8 & 0x0F);
        }
        for min in [2u32, 4, 8] {
            let mask = ((1u32 << min) - 1) as u8;
            let d: Vec<u8> = data.iter().map(|v| v & mask).collect();
            assert_eq!(lzw_decode(min, &lzw_encode(min, &d)), d, "min code size {min}");
        }
        assert_eq!(lzw_decode(2, &lzw_encode(2, &[1])), vec![1]);
    }

    #[test]
    fn gif_structure() {
        let mut img = Image::new(4, 4);
        img.pixels[0..4].copy_from_slice(&[255, 0, 0, 255]);
        let gif = encode(4, 4, &[GifFrame { image: &img, duration_ms: 120 }, GifFrame { image: &img, duration_ms: 80 }], 0);
        assert_eq!(&gif[..6], b"GIF89a");
        assert_eq!(*gif.last().unwrap(), 0x3B);
    }
}

mod export_inflate {
    use crate::export::deflate::zlib_compress;
    #[allow(unused_imports)]
    use crate::export::inflate::*;

    #[test]
    fn roundtrip_and_rejects_garbage() {
        let mut data = Vec::new();
        for i in 0..50_000u32 {
            data.push((i * 7 % 13) as u8);
        }
        assert_eq!(zlib_decompress(&zlib_compress(&data), 1 << 20).unwrap(), data);
        assert!(zlib_decompress(&zlib_compress(&data), 100).is_err(), "limit enforced");
        let mut bad = zlib_compress(&data);
        let mid = bad.len() / 2;
        bad[mid] ^= 0xFF;
        assert!(zlib_decompress(&bad, 1 << 20).is_err());
        assert!(zlib_decompress(b"\x78\x9c\xff\xff\xff", 1000).is_err());
        assert!(zlib_decompress(b"", 1000).is_err());
    }

    #[test]
    fn decodes_dynamic_huffman_from_reference_encoder() {
        // Produced by the reference zlib (level 9, BTYPE=2 dynamic Huffman block).
        let z = [
            0x78, 0xda, 0xd5, 0xcc, 0xc1, 0x0d, 0x80, 0x20, 0x10, 0x44, 0xd1, 0x56, 0xc6, 0x36, 0xac, 0x82, 0xc4, 0x0a, 0x10, 0x36, 0xb0,
            0xc9, 0xc2, 0x24, 0x2c, 0x18, 0xcb, 0x57, 0xcb, 0xf0, 0xf8, 0x0e, 0xff, 0x07, 0xbd, 0xc5, 0x70, 0xcc, 0x95, 0x95, 0xb8, 0xf8,
            0xa1, 0xca, 0xe0, 0x8e, 0x68, 0xa7, 0x64, 0xa2, 0x73, 0xb4, 0x68, 0x18, 0x5c, 0xa5, 0x76, 0x71, 0x47, 0x93, 0x19, 0xcd, 0x34,
            0x81, 0x29, 0xd9, 0x72, 0x65, 0x7f, 0x03, 0x2d, 0x75, 0x42, 0x9a, 0xfa, 0xe7, 0x0d, 0xe1, 0x37, 0xd3, 0x07, 0xcd, 0x99, 0x5f,
            0x8c,
        ];
        let text = "Pixel Studio voxel hero: albedo normal roughness metallic occlusion height emission! ".repeat(3);
        assert_eq!(zlib_decompress(&z, 1000).unwrap(), text.as_bytes());
    }
}

mod export_png {
    #[allow(unused_imports)]
    use crate::export::png::*;
    use crate::renderer::Image;

    #[test]
    fn png_roundtrip() {
        let mut img = Image::new(37, 23);
        for (i, p) in img.pixels.iter_mut().enumerate() {
            *p = ((i * 7) ^ (i / 13)) as u8;
        }
        let png = encode(&img);
        assert_eq!(decode(&png).unwrap(), img);
        let mut bad = png.clone();
        bad[40] ^= 1;
        assert!(decode(&bad).is_err());
        assert!(decode(b"nope").is_err());
    }
}

mod ffi {
    #[allow(unused_imports)]
    use crate::ffi::*;

    #[test]
    fn handles_are_validated() {
        let h = create_engine(8, 8);
        let ok = call(h, br#"{"op":"status"}"#, &[]);
        let len = u32::from_le_bytes(ok[..4].try_into().unwrap()) as usize;
        assert!(std::str::from_utf8(&ok[4..4 + len]).unwrap().contains("\"ok\":true"));
        assert!(destroy_engine(h));
        assert!(!destroy_engine(h));
        let gone = call(h, br#"{"op":"status"}"#, &[]);
        assert!(String::from_utf8_lossy(&gone).contains("invalid_handle"));
        assert!(String::from_utf8_lossy(&call(0xDEAD_BEEF, b"{}", &[])).contains("invalid_handle"));
    }
}

mod gamedev_atlas {
    #[allow(unused_imports)]
    use crate::gamedev::atlas::*;
    use crate::renderer::Image;

    fn dot(w: u32, h: u32, x: u32, y: u32, c: u8) -> Image {
        let mut i = Image::new(w, h);
        let o = ((y * w + x) * 4) as usize;
        i.pixels[o..o + 4].copy_from_slice(&[c, 0, 0, 255]);
        i
    }

    #[test]
    fn trims_dedupes_and_never_overlaps() {
        let mut sprites = vec![dot(32, 32, 5, 7, 10), dot(32, 32, 5, 7, 10), dot(32, 32, 20, 3, 99)];
        for k in 0..20 {
            let mut img = Image::new(24, 24);
            for y in 0..(4 + k % 9) {
                for x in 0..(3 + k % 7) {
                    let o = ((y * 24 + x) * 4) as usize;
                    img.pixels[o..o + 4].copy_from_slice(&[k as u8, 1, 2, 255]);
                }
            }
            sprites.push(img);
        }
        let a = pack(&sprites, true, 1, 1, false).unwrap();
        assert_eq!(a.placements[0].frame, a.placements[1].frame, "duplicates share pixels");
        assert_eq!(a.placements[1].same_as, Some(0));
        assert_eq!((a.placements[2].offset_x, a.placements[2].offset_y, a.placements[2].frame.w), (20, 3, 1));
        let rects: Vec<Rect> = a.placements.iter().filter(|p| p.same_as.is_none()).map(|p| p.frame).collect();
        for (i, r) in rects.iter().enumerate() {
            assert!(r.x + r.w <= a.image.width && r.y + r.h <= a.image.height);
            for s in &rects[i + 1..] {
                let overlap = r.x < s.x + s.w && r.x + r.w > s.x && r.y < s.y + s.h && r.y + r.h > s.y;
                assert!(!overlap, "{r:?} overlaps {s:?}");
            }
        }
        // Pixel content preserved.
        let p = &a.placements[2];
        assert_eq!(a.image.get(p.frame.x, p.frame.y).r, 99);
        let pot = pack(&sprites, true, 0, 0, true).unwrap();
        assert!(pot.image.width.is_power_of_two() && pot.image.height.is_power_of_two());
    }
}

mod gamedev_light2d {
    #[allow(unused_imports)]
    use crate::gamedev::light2d::*;
    use crate::renderer::Image;

    #[test]
    fn lights_brighten_near_and_facing_pixels() {
        let mut img = Image::new(16, 16);
        for p in img.pixels.as_chunks_mut::<4>().0 {
            *p = [128, 128, 128, 255];
        }
        let flat = {
            let mut n = Image::new(16, 16);
            for p in n.pixels.as_chunks_mut::<4>().0 {
                *p = [128, 128, 255, 255];
            }
            n
        };
        let s = LightSettings {
            ambient: [0.05, 0.05, 0.08],
            lights: vec![Light2D { x: 2.0, y: 8.0, z: 6.0, color: [1.0, 0.6, 0.3], intensity: 2.0, radius: 12.0 }],
            bands: 4,
            normal_strength: 1.0,
        };
        let out = light(&img, &flat, &s);
        assert!(out.get(2, 8).r > out.get(13, 8).r);
        assert!(out.get(2, 8).r > out.get(2, 8).b, "warm light");
        let distinct: std::collections::HashSet<u8> = (0..16).map(|x| out.get(x, 8).r).collect();
        assert!(distinct.len() <= 6, "banded light keeps few steps");
    }
}

mod gamedev_rotsprite {
    #[allow(unused_imports)]
    use crate::gamedev::rotsprite::*;

    #[test]
    fn quarter_turn_and_no_new_colors() {
        let (w, h) = (9, 9);
        let mut src = vec![0u32; w * h];
        for x in 4..9 {
            src[4 * w + x] = 0xFF0000FF; // ray pointing right from the center
        }
        let r = rotate(&src, w, h, 90.0, 4.5, 4.5);
        assert_eq!(r[8 * w + 4], 0xFF0000FF, "clockwise 90° turns right into down");
        assert_eq!(r[4 * w + 8], 0);
        let r30 = rotate(&src, w, h, 30.0, 4.5, 4.5);
        assert!(r30.iter().all(|&v| v == 0 || v == 0xFF0000FF));
        assert!(r30.iter().filter(|&&v| v != 0).count() >= 4);
    }
}

mod gamedev_tiles {
    #[allow(unused_imports)]
    use crate::gamedev::tiles::*;
    use crate::pixel::Rgba;

    #[test]
    fn there_are_47_blob_masks() {
        let m = blob_masks();
        assert_eq!(m.len(), 47);
        assert_eq!(m[0], 0);
        assert_eq!(*m.last().unwrap(), 255);
        assert_eq!(reduce(NE), 0, "lonely corner is ignored");
    }

    #[test]
    fn autotile_picks_edges_and_center() {
        let fill = vec![Rgba::new(80, 160, 60, 255).to_pixel(); 64];
        let style = BlobStyle {
            inset: 1,
            edge: 1,
            light: Rgba::new(150, 220, 100, 255),
            dark: Rgba::new(40, 100, 40, 255),
            outline: Rgba::new(20, 40, 30, 255),
            lip: 1,
            lip_color: Rgba::new(100, 60, 40, 255),
            rounded: true,
        };
        let tiles = synthesize_blob(&fill, 8, 8, &style);
        assert_eq!(tiles.len(), 47);
        let ts = Tileset { name: "g".into(), tile_w: 8, tile_h: 8, kind: TilesetKind::Blob, tiles };
        let mut map = Tilemap::new("m", 0, 5, 5);
        for y in 1..4 {
            for x in 1..4 {
                map.terrain[y * 5 + x] = true;
            }
        }
        map.autotile();
        let masks = blob_masks();
        let center = map.cells[2 * 5 + 2] - 1;
        assert_eq!(masks[center as usize], 255);
        let top = map.cells[5 + 2] - 1;
        assert_eq!(masks[top as usize], reduce(E | SE | S | SW | W));
        assert_eq!(map.cells[0], 0);
        let img = map.render(&ts);
        assert_eq!(img.width, 40);
        // Top edge is lit, interior keeps the fill.
        assert_eq!(img.get(20, 10), Rgba::new(150, 220, 100, 255));
        assert_eq!(img.get(20, 20), Rgba::new(80, 160, 60, 255));
        assert_eq!(img.get(20, 8).a, 0, "inset above the top edge stays open");
    }
}

mod pixel_color {
    #[allow(unused_imports)]
    use crate::pixel::color::*;

    #[test]
    fn hex_roundtrip() {
        assert_eq!(Rgba::parse_hex("#D94B42"), Some(Rgba::new(0xD9, 0x4B, 0x42, 255)));
        assert_eq!(Rgba::parse_hex("fff"), Some(Rgba::new(255, 255, 255, 255)));
        assert_eq!(Rgba::parse_hex("#11223380").unwrap().to_hex(), "#11223380");
        assert_eq!(Rgba::parse_hex("transparent"), Some(Rgba::TRANSPARENT));
        assert_eq!(Rgba::parse_hex("#12345"), None);
        assert_eq!(Rgba::parse_hex("#GG0000"), None);
        assert_eq!(Rgba::parse_hex("#ééé"), None);
        let c = Rgba::new(1, 2, 3, 4);
        assert_eq!(Rgba::from_u32(c.to_u32()), c);
    }
}

mod pixel_grid {
    #[allow(unused_imports)]
    use crate::pixel::grid::*;
    use std::sync::Arc;

    #[test]
    fn sparse_set_get() {
        let mut g = PixelGrid::new(40, 20);
        assert_eq!(g.tiles().len(), 3 * 2);
        assert!(g.set(39, 19, 7));
        assert!(!g.set(39, 19, 7));
        assert_eq!(g.get(39, 19), 7);
        assert_eq!(g.get(40, 19), 0);
        assert!(!g.set(-1, 0, 3));
        assert_eq!(g.allocated_tiles(), 1);
        assert_eq!(g.bounds(), Some((39, 19, 1, 1)));
        g.set(39, 19, 0);
        g.compact();
        assert_eq!(g.allocated_tiles(), 0);
        assert!(g.is_empty());
    }

    #[test]
    fn copy_on_write_sharing() {
        let mut a = PixelGrid::new(32, 32);
        a.set(1, 1, 5);
        a.set(20, 20, 6);
        let mut b = a.clone();
        b.set(1, 1, 9);
        assert_eq!(a.get(1, 1), 5);
        assert_eq!(b.get(1, 1), 9);
        // The untouched tile is still physically shared.
        assert!(Arc::ptr_eq(a.tile(1, 1).unwrap(), b.tile(1, 1).unwrap()));
        assert!(!Arc::ptr_eq(a.tile(0, 0).unwrap(), b.tile(0, 0).unwrap()));
    }
}

mod pixelart {
    use crate::pixel::Rgba;
    #[allow(unused_imports)]
    use crate::pixelart::*;
    use crate::renderer::Image;

    #[test]
    fn oklab_roundtrip_and_ramps() {
        for c in [Rgba::new(255, 0, 0, 255), Rgba::new(18, 200, 77, 255), Rgba::new(250, 250, 250, 255), Rgba::new(10, 10, 40, 255)] {
            let back = from_oklab(oklab(c), 255);
            for (a, b) in [(c.r, back.r), (c.g, back.g), (c.b, back.b)] {
                assert!((a as i32 - b as i32).abs() <= 1, "{c:?} -> {back:?}");
            }
        }
        let r = ramp(Rgba::parse_hex("#3FA34D").unwrap(), 5, 20.0, 0.8);
        assert_eq!(r.len(), 5);
        let l: Vec<f32> = r.iter().map(|c| lch(*c)[0]).collect();
        assert!(l.windows(2).all(|w| w[0] < w[1]), "ramp goes dark → light: {l:?}");
        let (hd, hb, hl) = (lch(r[0])[2], lch(r[2])[2], lch(r[4])[2]);
        assert!(hd > hb && hl < hb, "shadows cooler, highlights warmer ({hd} {hb} {hl})");
        assert!(palette_by_name("endesga-32").unwrap().len() == 32);
        assert!(palette_by_name("Sweetie-16").is_some());
    }

    #[test]
    fn quantize_and_dither() {
        let mut img = Image::new(16, 1);
        for x in 0..16u32 {
            let v = (x * 17) as u8;
            img.pixels[(x * 4) as usize..(x * 4 + 4) as usize].copy_from_slice(&[v, v, v, 255]);
        }
        let pal = median_cut(&image_samples(&img), 4);
        assert!(pal.len() <= 4);
        let mut q = img.clone();
        quantize_image(&mut q, &pal, Dither::None, 0.0);
        let distinct: std::collections::HashSet<_> = q.pixels.chunks(4).map(|p| p.to_vec()).collect();
        assert!(distinct.len() <= 4);
        let mut d = img.clone();
        quantize_image(&mut d, &[Rgba::new(0, 0, 0, 255), Rgba::new(255, 255, 255, 255)], Dither::Bayer4, 255.0);
        assert!(d.pixels.chunks(4).any(|p| p[0] == 0) && d.pixels.chunks(4).any(|p| p[0] == 255));
        assert!(dither_pick(0, 0, 1) && !dither_pick(0, 0, 0));
    }

    #[test]
    fn outline_and_normals() {
        let mut img = Image::new(8, 8);
        for y in 2..6 {
            for x in 2..6 {
                let o = (y * 8 + x) * 4;
                img.pixels[o..o + 4].copy_from_slice(&[200, 80, 60, 255]);
            }
        }
        let n = normal_map(&img, 4.0, 2.0);
        assert!(n.get(2, 3).r < 128 && n.get(5, 3).r > 128, "left edge faces left, right edge faces right");
        outline_image(&mut img, None, false);
        assert_eq!(img.get(1, 3).a, 255);
        assert_eq!(img.get(1, 1).a, 0, "no corners by default");
        assert!(img.get(1, 3).r < 100);
    }
}

mod pixelart_charart {
    use crate::pixel::Rgba;
    #[allow(unused_imports)]
    use crate::pixelart::charart::*;
    use crate::renderer::Image;

    fn disc(size: u32, c: Rgba) -> Image {
        let mut img = Image::new(size, size);
        let r = size as f32 / 2.0 - 1.0;
        for y in 0..size {
            for x in 0..size {
                if (x as f32 + 0.5 - size as f32 / 2.0).hypot(y as f32 + 0.5 - size as f32 / 2.0) <= r {
                    let o = ((y * size + x) * 4) as usize;
                    img.pixels[o..o + 4].copy_from_slice(&[c.r, c.g, c.b, 255]);
                }
            }
        }
        img
    }

    #[test]
    fn auto_shade_lights_top_left_and_shadows_bottom_right() {
        let base = Rgba::new(240, 150, 190, 255);
        let mut img = disc(24, base);
        let n = auto_shade(&mut img, &ShadeSettings::default(), &[], 0);
        assert!(n > 20);
        let lum = |c: Rgba| c.r as u32 + c.g as u32 + c.b as u32;
        let tl = img.get(6, 6);
        let br = img.get(17, 17);
        assert!(lum(tl) > lum(br), "lit side brighter: {tl:?} vs {br:?}");
        assert_eq!(img.get(12, 12), base, "interior keeps the base color");
        let distinct: std::collections::HashSet<[u8; 4]> = img.pixels.as_chunks::<4>().0.iter().copied().collect();
        assert!(distinct.len() <= 5, "only ramp tones (+ transparent)");
    }

    #[test]
    fn occluders_cast_shadows_and_small_regions_are_kept() {
        let base = Rgba::new(250, 220, 200, 255);
        let mut img = Image::new(16, 16);
        for p in img.pixels.as_chunks_mut::<4>().0 {
            *p = [base.r, base.g, base.b, 255];
        }
        // Tiny 2-px detail stays untouched.
        img.pixels[(8 * 16 + 8) * 4..(8 * 16 + 8) * 4 + 4].copy_from_slice(&[10, 10, 10, 255]);
        img.pixels[(8 * 16 + 9) * 4..(8 * 16 + 9) * 4 + 4].copy_from_slice(&[10, 10, 10, 255]);
        let mut occ = vec![false; 256];
        for x in 0..16 {
            occ[2 * 16 + x] = true; // a "bang" row above
        }
        let s = ShadeSettings { form: 0.0, ..Default::default() };
        auto_shade(&mut img, &s, &occ, 2);
        let lum = |c: Rgba| c.r as u32 + c.g as u32 + c.b as u32;
        assert!(lum(img.get(8, 3)) < lum(img.get(8, 12)), "shadow just below the occluder");
        assert_eq!(img.get(8, 8), Rgba::new(10, 10, 10, 255));
    }

    #[test]
    fn selout_gradient_map_sparkle_and_cleanup() {
        let mut img = disc(12, Rgba::new(255, 170, 200, 255));
        outline_ex(&mut img, None, false, true, 0.5, (-1.0, -1.0), false);
        let top = img.get(6, 0);
        let bottom = img.get(6, 11);
        assert!(top.a == 255 && bottom.a == 255);
        assert!(top.r as u32 + top.g as u32 > bottom.r as u32 + bottom.g as u32, "lit outline is lighter");
        let mut g = disc(8, Rgba::new(200, 200, 200, 255));
        gradient_map(&mut g, &[Rgba::new(0, 0, 80, 255), Rgba::new(120, 200, 255, 255)], 1.0);
        let c = g.get(4, 4);
        assert!(c.b > c.r, "graded toward blue");
        assert!(sparkle_shape(4, "star4").len() > 12);
        let mut line = Image::new(5, 5);
        for (x, y) in [(0, 0), (1, 0), (1, 1), (2, 1)] {
            let o = (y * 5 + x) * 4;
            line.pixels[o..o + 4].copy_from_slice(&[0, 0, 0, 255]);
        }
        assert_eq!(pixel_cleanup(&mut line, None), 1, "one corner double removed");
    }
}

mod project {
    use crate::animation::Tag;
    #[allow(unused_imports)]
    use crate::project::*;
    use std::sync::Arc;

    #[test]
    fn duplicate_shares_until_edit() {
        let mut d = Document::new(16, 16);
        let l = d.active_layer;
        let f0 = d.active_frame;
        d.cel_mut(l, f0).set(3, 3, 1);
        let f1 = d.duplicate_frame(0, 1);
        assert!(Arc::ptr_eq(&d.cels[&(l, f0)], &d.cels[&(l, f1)]));
        d.cel_mut(l, f1).set(3, 3, 2);
        assert_eq!(d.cel(l, f0).unwrap().get(3, 3), 1);
        assert_eq!(d.cel(l, f1).unwrap().get(3, 3), 2);
    }

    #[test]
    fn removing_frames_repairs_tags() {
        let mut d = Document::new(8, 8);
        let ids: Vec<u32> = (0..4).map(|i| d.insert_frame(i + 1, 100)).collect();
        d.tags.push(Tag {
            name: "walk".into(),
            from_id: ids[0],
            to_id: ids[2],
            direction: crate::animation::Direction::Forward,
            repeat: 0,
        });
        let idx = d.frame_index(ids[2]).unwrap();
        d.remove_frame(idx);
        assert_eq!(d.tags[0].to_id, ids[1]);
    }
}

mod raster {
    use crate::pixel::PixelGrid;
    #[allow(unused_imports)]
    use crate::raster::*;

    fn dump(g: &PixelGrid) -> String {
        let mut s = String::new();
        for y in 0..g.height() as i64 {
            for x in 0..g.width() as i64 {
                s.push(if g.get(x, y) == 0 { '.' } else { char::from_digit(g.get(x, y), 10).unwrap() });
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn line_is_inclusive_and_symmetric() {
        let mut a = PixelGrid::new(8, 8);
        let mut b = PixelGrid::new(8, 8);
        assert_eq!(line(&mut a, 0, 0, 7, 3, 1), 8);
        line(&mut b, 7, 3, 0, 0, 1);
        assert_eq!(a.get(0, 0), 1);
        assert_eq!(a.get(7, 3), 1);
        assert_eq!(b.get(0, 0), 1);
    }

    #[test]
    fn rect_outline_and_fill() {
        let mut g = PixelGrid::new(5, 4);
        assert_eq!(rectangle(&mut g, Region { x: 0, y: 0, w: 5, h: 4 }, 1, false), 14);
        assert_eq!(dump(&g), "11111\n1...1\n1...1\n11111\n");
        assert_eq!(fill(&mut g, 2, 2, 2, true), 6);
        assert_eq!(dump(&g), "11111\n12221\n12221\n11111\n");
    }

    #[test]
    fn flood_fill_respects_walls() {
        let mut g = PixelGrid::new(6, 3);
        line(&mut g, 2, 0, 2, 2, 1);
        assert_eq!(fill(&mut g, 0, 0, 3, true), 6);
        assert_eq!(g.get(4, 1), 0);
        assert_eq!(fill(&mut g, 4, 1, 3, false), 9);
    }

    #[test]
    fn ellipse_is_symmetric() {
        let mut g = PixelGrid::new(9, 7);
        ellipse(&mut g, Region { x: 0, y: 0, w: 9, h: 7 }, 1, false);
        let s = dump(&g);
        let rows: Vec<&str> = s.lines().collect();
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(*row, rows[rows.len() - 1 - i]);
            assert_eq!(row.chars().rev().collect::<String>(), *row);
        }
        assert_eq!(g.get(4, 3), 0, "outline must be hollow");
    }

    #[test]
    fn flips_and_rotations() {
        let mut g = PixelGrid::new(3, 3);
        g.set(0, 0, 1);
        g.set(1, 0, 2);
        let full = Region::full(&g);
        flip_horizontal(&mut g, full);
        assert_eq!(dump(&g), ".21\n...\n...\n");
        flip_vertical(&mut g, full);
        assert_eq!(dump(&g), "...\n...\n.21\n");
        rotate(&mut g, full, 90).unwrap();
        assert_eq!(dump(&g), "...\n2..\n1..\n");
        rotate(&mut g, full, 270).unwrap();
        assert_eq!(dump(&g), "...\n...\n.21\n");
        let mut wide = PixelGrid::new(4, 2);
        assert!(rotate(&mut wide, Region::full(&PixelGrid::new(4, 2)), 90).is_err());
        assert!(rotate(&mut wide, Region::full(&PixelGrid::new(4, 2)), 180).is_ok());
    }
}

mod renderer {
    use crate::pixel::Rgba;
    use crate::project::Document;
    #[allow(unused_imports)]
    use crate::renderer::*;

    #[test]
    fn blend_rules() {
        let mut d = [0u8, 0, 0, 0];
        blend_over(&mut d, Rgba::new(10, 20, 30, 255), 255);
        assert_eq!(d, [10, 20, 30, 255]);
        let mut d = [0u8, 0, 255, 255];
        blend_over(&mut d, Rgba::new(255, 0, 0, 255), 128);
        assert_eq!(d, [128, 0, 127, 255]);
        let mut d = [9u8, 9, 9, 255];
        blend_over(&mut d, Rgba::new(255, 0, 0, 0), 255);
        assert_eq!(d, [9, 9, 9, 255]);
    }

    #[test]
    fn layer_order_and_visibility() {
        let mut doc = Document::new(4, 4);
        let bottom = doc.active_layer;
        let top = doc.insert_layer("top", 1);
        let f = doc.active_frame;
        let red = Rgba::new(255, 0, 0, 255).to_pixel();
        let blue = Rgba::new(0, 0, 255, 255).to_pixel();
        doc.cel_mut(bottom, f).set(1, 1, red);
        doc.cel_mut(top, f).set(1, 1, blue);
        let img = render(&doc, 0, &RenderOptions::default());
        assert_eq!(img.get(1, 1), Rgba::new(0, 0, 255, 255));
        doc.layers[1].visible = false;
        let img = render(&doc, 0, &RenderOptions::default());
        assert_eq!(img.get(1, 1), Rgba::new(255, 0, 0, 255));
        assert_eq!(img.scaled(3).get(5, 5), Rgba::new(255, 0, 0, 255));
    }
}

mod storage {
    use crate::animation::Direction;
    use crate::animation::Tag;
    use crate::gamedev::Slice;
    use crate::gamedev::SliceKey;
    use crate::gamedev::tiles::Tilemap;
    use crate::gamedev::tiles::Tileset;
    use crate::gamedev::tiles::TilesetKind;
    use crate::history::LogEntry;
    use crate::pixel::Rgba;
    use crate::project::Document;
    use crate::renderer::{RenderOptions, render};
    #[allow(unused_imports)]
    use crate::storage::*;
    use std::sync::Arc;

    fn sample() -> Document {
        let mut d = Document::new(40, 24);
        let l0 = d.active_layer;
        let l1 = d.insert_layer("Weapon", 1);
        d.layers[1].opacity = 200;
        let f0 = d.active_frame;
        let red = Rgba::new(217, 75, 66, 255).to_pixel();
        for i in 0..30 {
            d.cel_mut(l0, f0).set(i, i % 24, red);
        }
        d.cel_mut(l1, f0).set(39, 23, 3);
        let f1 = d.duplicate_frame(0, 1);
        d.cel_mut(l1, f1).set(0, 0, 5);
        d.frames[1].duration_ms = 80;
        d.tags.push(Tag { name: "Idle".into(), from_id: f0, to_id: f1, direction: Direction::PingPong, repeat: 0 });
        d.name = "Knight".into();
        d.layers[1].blend = crate::layers::Blend::Screen;
        d.slices.push(Slice {
            name: "hitbox".into(),
            color: 0xFF0000FF,
            keys: vec![SliceKey { frame_id: f0, x: 2, y: 3, w: 10, h: 12, pivot: Some((5, 11)) }],
        });
        d.tilesets.push(Arc::new(Tileset {
            name: "grass".into(),
            tile_w: 4,
            tile_h: 4,
            kind: TilesetKind::Plain,
            tiles: vec![vec![0x22AA33FF; 16], vec![0; 16]],
        }));
        let mut map = Tilemap::new("level", 0, 6, 3);
        map.cells[4] = 1;
        map.terrain[5] = true;
        d.tilemaps.push(Arc::new(map));
        d
    }

    #[test]
    fn roundtrip_preserves_everything() {
        let d = sample();
        let log = vec![LogEntry {
            revision: 7,
            actor: "claude".into(),
            op: "set_pixel".into(),
            operation_id: Some("op_1".into()),
            changed_pixels: 1,
            timestamp_ms: 42,
        }];
        let bytes = save(&d, &log);
        let loaded = load(&bytes).unwrap();
        let e = &loaded.doc;
        assert_eq!(e.name, "Knight");
        assert_eq!((e.width, e.height), (40, 24));
        assert_eq!(e.layers, d.layers);
        assert_eq!(e.frames, d.frames);
        assert_eq!(e.tags, d.tags);
        assert_eq!(e.palette, d.palette);
        assert_eq!(e.slices, d.slices);
        assert_eq!(e.tilesets, d.tilesets);
        assert_eq!(e.tilemaps, d.tilemaps);
        assert_eq!(loaded.log, log);
        for i in 0..d.frames.len() {
            assert_eq!(render(e, i, &RenderOptions::default()), render(&d, i, &RenderOptions::default()));
        }
        // Saving the loaded document is byte-identical (deterministic format).
        assert_eq!(save(e, &loaded.log), bytes);
    }

    #[test]
    fn rejects_corruption() {
        let bytes = save(&sample(), &[]);
        for i in [0usize, 9, bytes.len() / 2, bytes.len() - 1] {
            let mut bad = bytes.clone();
            bad[i] ^= 0x55;
            assert!(load(&bad).is_err(), "flip at {i} not detected");
        }
        assert!(load(&bytes[..bytes.len() - 3]).is_err());
        assert!(load(b"hello").is_err());
    }

    #[test]
    fn duplicate_frames_are_nearly_free() {
        let mut d = sample();
        let one = save(&d, &[]).len();
        for _ in 0..50 {
            d.duplicate_frame(0, 1);
        }
        let many = save(&d, &[]).len();
        // Each extra frame only stores tile references, never pixel data again.
        assert!(many - one <= 50 * 72, "grew by {}", many - one);
    }
}

mod voxel_material {
    #[allow(unused_imports)]
    use crate::voxel::material::*;
    use std::sync::Arc;

    #[test]
    fn generated_sets_tile_and_vary() {
        for kind in TEXTURE_KINDS {
            let set = generate_set("t", kind, 32, 4, 7, 0.6, 0.5);
            assert_eq!(set.albedo.data.len(), 32 * 32 * 4);
            let first = set.height.data[0];
            assert!(set.height.data.chunks(4).any(|p| p[0] != first), "{kind} must not be flat");
            // Sampling wraps.
            assert_eq!(set.normal.sample(32.0, 0.0), set.normal.sample(0.0, 0.0));
        }
        assert_eq!(slot_by_name("AO"), Some(MapSlot::Occlusion));
        assert_eq!(slot_by_name("basecolor"), Some(MapSlot::Albedo));
    }

    #[test]
    fn surface_sampling_uses_maps() {
        let set = generate_set("m", "stone", 16, 2, 1, 0.8, 0.6);
        let textures = vec![Arc::new(set.albedo), Arc::new(set.normal)];
        let mut mat = Material::new("stone");
        mat.maps[MapSlot::Albedo as usize] = Some(0);
        mat.maps[MapSlot::Normal as usize] = Some(1);
        let a = sample_surface(&mat, &textures, [3.5, 5.5, 1.0], 2);
        let b = sample_surface(&mat, &textures, [9.5, 2.5, 1.0], 2);
        assert!(a.albedo_mul != b.albedo_mul || a.normal_ts != b.normal_ts);
        assert!((a.normal_ts.length() - 1.0).abs() < 1e-3);
    }
}

mod voxel_math {
    #[allow(unused_imports)]
    use crate::voxel::math::*;

    #[test]
    fn rotations() {
        let r = Affine::rotation_euler(Vec3::new(0.0, 90.0, 0.0));
        let p = r.apply(Vec3::new(0.0, 0.0, 1.0));
        assert!((p.x - 1.0).abs() < 1e-5 && p.z.abs() < 1e-5);
        let about = Affine::rotation_about(Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 90.0));
        let q = about.apply(Vec3::new(2.0, 0.0, 0.0));
        assert!((q.x - 1.0).abs() < 1e-5 && (q.y - 1.0).abs() < 1e-5);
        let chain = Affine::translation(Vec3::new(5.0, 0.0, 0.0)).then(&about);
        assert!((chain.apply(Vec3::new(2.0, 0.0, 0.0)).x - 6.0).abs() < 1e-5);
    }
}

mod voxel_model {
    use crate::voxel::Vec3;
    use crate::voxel::Voxel;
    use crate::voxel::VoxelModel;
    #[allow(unused_imports)]
    use crate::voxel::model::*;

    #[test]
    fn sparse_voxels() {
        let mut m = VoxelModel::new("t", [64, 64, 64]);
        let v = Voxel { rgba: 0xFF0000FF, material: 1, bone: 2 };
        let before = m.version;
        assert!(m.set(17, 3, 40, v));
        assert!(m.version > before);
        assert!(!m.set(17, 3, 40, v));
        assert_eq!(m.get(17, 3, 40), v);
        assert_eq!(m.get(64, 0, 0), Voxel::EMPTY);
        assert!(!m.set(-1, 0, 0, v));
        assert_eq!(m.count(), 1);
        assert_eq!(m.bounds(), Some(([17, 3, 40], [17, 3, 40])));
        let mut seen = vec![];
        m.for_each(|x, y, z, _| seen.push((x, y, z)));
        assert_eq!(seen, vec![(17, 3, 40)]);
        m.set(17, 3, 40, Voxel::EMPTY);
        m.compact();
        assert!(m.chunks.is_empty());
    }

    #[test]
    fn shapes() {
        let cap = Shape::Capsule { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 0.0, 0.0), ra: 2.0, rb: 1.0 };
        assert!(cap.contains(Vec3::new(0.0, 1.9, 0.0)));
        assert!(!cap.contains(Vec3::new(10.0, 1.9, 0.0)));
        let e = Shape::Ellipsoid { center: Vec3::new(5.0, 5.0, 5.0), radii: Vec3::new(4.0, 2.0, 2.0) };
        assert!(e.contains(Vec3::new(8.5, 5.0, 5.0)));
        assert!(!e.contains(Vec3::new(5.0, 7.5, 5.0)));
        let n = value_noise(Vec3::new(1.3, 2.7, 0.2), 7);
        assert!((0.0..1.0).contains(&n));
    }
}

mod voxel_render {
    use crate::pixel::Rgba;
    use crate::renderer::Image;
    use crate::voxel::Material;
    use crate::voxel::Vec3;
    use crate::voxel::Voxel;
    use crate::voxel::VoxelModel;
    use crate::voxel::model::Shape;
    #[allow(unused_imports)]
    use crate::voxel::render::*;

    fn fill(m: &mut VoxelModel, s: Shape, rgba: Rgba, material: u8) {
        let (lo, hi) = s.aabb();
        for z in lo.z.floor() as i32..=hi.z.ceil() as i32 {
            for y in lo.y.floor() as i32..=hi.y.ceil() as i32 {
                for x in lo.x.floor() as i32..=hi.x.ceil() as i32 {
                    if s.contains(Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5)) {
                        m.set(x, y, z, Voxel { rgba: rgba.to_pixel(), material, bone: 0 });
                    }
                }
            }
        }
    }

    fn ball() -> VoxelModel {
        let mut m = VoxelModel::new("ball", [32, 32, 32]);
        fill(
            &mut m,
            Shape::Ellipsoid { center: Vec3::new(16.0, 16.0, 16.0), radii: Vec3::new(10.0, 10.0, 10.0) },
            Rgba::new(200, 60, 40, 255),
            0,
        );
        m.origin = Vec3::new(16.0, 16.0, 16.0);
        m
    }

    fn cam(yaw: f32) -> Camera {
        Camera { yaw, pitch: 30.0, scale: 3.0, width: 96, height: 96, anchor_x: 48.0, anchor_y: 48.0 }
    }

    fn lum(c: Rgba) -> u32 {
        c.r as u32 + c.g as u32 + c.b as u32
    }

    #[test]
    fn surface_is_shell_with_outward_normals_and_corner_ao() {
        let m = ball();
        let p = prepare(&m);
        assert!(!p.faces.is_empty());
        assert!(p.faces.len() < m.count() * 6);
        for f in &p.faces {
            let center = Vec3::new(f.pos[0] as f32 + 0.5 - 16.0, f.pos[1] as f32 + 0.5 - 16.0, f.pos[2] as f32 + 0.5 - 16.0);
            assert!(f.smooth.dot(center) > 0.0, "normal must point outward");
            assert!((0.0..=1.0).contains(&f.ao));
        }
        // Inner corner of an L shape gets occluded corners.
        let mut l = VoxelModel::new("l", [8, 8, 8]);
        fill(&mut l, Shape::Box { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(4.0, 1.0, 4.0) }, Rgba::new(255, 255, 255, 255), 0);
        fill(&mut l, Shape::Box { min: Vec3::new(0.0, 1.0, 0.0), max: Vec3::new(1.0, 4.0, 4.0) }, Rgba::new(255, 255, 255, 255), 0);
        let pl = prepare(&l);
        let floor = pl.faces.iter().find(|f| f.dir == 2 && f.pos == [1, 0, 2]).expect("floor face next to the wall");
        assert!(floor.corner_ao.iter().any(|&a| a < 1.0) && floor.corner_ao.contains(&1.0));
    }

    #[test]
    fn renders_lit_and_passes_deterministically() {
        let m = ball();
        let p = prepare(&m);
        let mats = vec![Material::new("default")];
        let mut s = RenderSettings::new(cam(0.0));
        let a = render(&m, &p, &mats, &[], &s);
        let b = render(&m, &p, &mats, &[], &s);
        assert_eq!(a, b);
        assert_eq!(a.get(48, 48).a, 255);
        assert_eq!(a.get(2, 2).a, 0);
        assert!(lum(a.get(38, 38)) > lum(a.get(60, 62)), "top-left key light");
        s.pass = Pass::Normal;
        let n = render(&m, &p, &mats, &[], &s);
        assert!(n.get(48, 48).b > 200, "center normal faces the viewer");
        s.pass = Pass::Lit;
        s.outline = Outline::Selective;
        s.supersample = 2;
        let o = render(&m, &p, &mats, &[], &s);
        let edge = (0..96).find(|&x| o.get(x, 48).a > 0).unwrap();
        assert!(lum(o.get(edge, 48)) < lum(o.get(edge + 3, 48)), "outline is darker than the interior");
    }

    #[test]
    fn cast_shadows_and_ground_shadow() {
        // A slab with a block hovering above it, lit from straight above.
        let mut m = VoxelModel::new("s", [40, 40, 40]);
        fill(&mut m, Shape::Box { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(40.0, 2.0, 40.0) }, Rgba::new(180, 180, 180, 255), 0);
        fill(&mut m, Shape::Box { min: Vec3::new(15.0, 12.0, 15.0), max: Vec3::new(25.0, 16.0, 25.0) }, Rgba::new(180, 180, 180, 255), 0);
        m.origin = Vec3::new(20.0, 0.0, 20.0);
        let p = prepare(&m);
        let mats = vec![Material::new("default")];
        let mut s =
            RenderSettings::new(Camera { yaw: 0.0, pitch: 89.0, scale: 2.0, width: 100, height: 100, anchor_x: 50.0, anchor_y: 50.0 });
        s.lighting.lights = vec![Light { dir: Vec3::new(0.0, 0.0, 1.0), color: Vec3::new(1.0, 1.0, 1.0), intensity: 3.0 }];
        s.shadows = 1.0;
        let img = render(&m, &p, &mats, &[], &s);
        // Top-down: the block is at the center; slab pixels just outside it are lit, the block casts on itself only.
        s.lighting.lights[0].dir = Vec3::new(0.7, 0.0, 0.7).normalized();
        let shadowed = render(&m, &p, &mats, &[], &s);
        s.shadows = 0.0;
        let unshadowed = render(&m, &p, &mats, &[], &s);
        let diff = (0..100 * 100).filter(|&i| shadowed.pixels[i * 4] + 8 < unshadowed.pixels[i * 4]).count();
        assert!(diff > 20, "the hovering block must cast a shadow ({diff} darker pixels)");
        assert_eq!(img.width, 100);
    }

    #[test]
    fn emission_blooms_into_a_halo() {
        let mut m = VoxelModel::new("glow", [16, 16, 16]);
        fill(
            &mut m,
            Shape::Ellipsoid { center: Vec3::new(8.0, 8.0, 8.0), radii: Vec3::new(3.0, 3.0, 3.0) },
            Rgba::new(255, 160, 40, 255),
            1,
        );
        m.origin = Vec3::new(8.0, 8.0, 8.0);
        let p = prepare(&m);
        let mut fire = Material::new("fire");
        fire.emission = 4.0;
        let mats = vec![Material::new("default"), fire];
        let mut s = RenderSettings::new(Camera { yaw: 0.0, pitch: 0.0, scale: 3.0, width: 64, height: 64, anchor_x: 32.0, anchor_y: 32.0 });
        let plain = render(&m, &p, &mats, &[], &s);
        s.bloom = 1.0;
        let glow = render(&m, &p, &mats, &[], &s);
        let halo = |img: &Image| (0..64 * 64).filter(|&i| img.pixels[i * 4 + 3] > 0).count();
        assert!(halo(&glow) > halo(&plain) + 50, "glow spills beyond the silhouette");
        s.lighting.points =
            vec![PointLight { pos: Vec3::new(8.0, 8.0, 14.0), color: Vec3::new(1.0, 0.5, 0.1), intensity: 8.0, radius: 20.0 }];
        s.bloom = 0.0;
        let warm = render(&m, &p, &[Material::new("default")], &[], &s);
        let cold = render(&m, &p, &[Material::new("default")], &[], &RenderSettings { lighting: Lighting::default(), ..s.clone() });
        assert!(warm.get(32, 32).r > cold.get(32, 32).r, "point light adds light");
    }

    #[test]
    fn picking_hits_the_front_face() {
        let m = ball();
        let p = prepare(&m);
        let s = RenderSettings { camera: Camera { pitch: 0.0, ..cam(0.0) }, ..RenderSettings::new(cam(0.0)) };
        let (pos, normal) = pick(&m, &p, &s, 48, 48).expect("hit");
        assert_eq!(normal, [0, 0, 1]);
        assert!(pos[2] >= 24);
        assert!(pick(&m, &p, &s, 1, 1).is_none());
    }
}

mod voxel_rig {
    use crate::voxel::Vec3;
    use crate::voxel::Voxel;
    use crate::voxel::VoxelModel;
    #[allow(unused_imports)]
    use crate::voxel::rig::*;

    fn two_bone_model() -> VoxelModel {
        let mut m = VoxelModel::new("arm", [32, 32, 32]);
        m.bones.push(Bone { name: "upper".into(), parent: None, head: Vec3::new(16.0, 30.0, 16.0), tail: Vec3::new(16.0, 20.0, 16.0) });
        m.bones.push(Bone { name: "lower".into(), parent: Some(0), head: Vec3::new(16.0, 20.0, 16.0), tail: Vec3::new(16.0, 10.0, 16.0) });
        m
    }

    #[test]
    fn interpolation_wraps() {
        let mut a = Animation::new("a", 8, 100);
        a.set_key(Key { bone: 0, frame: 0.0, rot: Vec3::ZERO, pos: Vec3::ZERO });
        a.set_key(Key { bone: 0, frame: 4.0, rot: Vec3::new(40.0, 0.0, 0.0), pos: Vec3::ZERO });
        assert_eq!(a.sample(0, 4.0).0.x, 40.0);
        assert!((a.sample(0, 2.0).0.x - 20.0).abs() < 1e-4);
        assert!((a.sample(0, 6.0).0.x - 20.0).abs() < 1e-4, "wraps from frame 4 back to 0");
        assert_eq!(a.sample(1, 3.0).0, Vec3::ZERO);
    }

    #[test]
    fn hierarchy_and_binding() {
        let mut m = two_bone_model();
        m.set(16, 25, 16, Voxel { rgba: 0xFFFFFFFF, ..Voxel::EMPTY });
        m.set(16, 12, 16, Voxel { rgba: 0xFFFFFFFF, ..Voxel::EMPTY });
        assert_eq!(auto_bind(&mut m, false), 1);
        assert_eq!(m.get(16, 12, 16).bone, 1);
        let mut a = Animation::new("bend", 1, 100);
        a.set_key(Key { bone: 0, frame: 0.0, rot: Vec3::new(0.0, 0.0, 90.0), pos: Vec3::ZERO });
        let w = pose(&m.bones, Some(&a), 0.0);
        // The child follows the parent's rotation around the shoulder.
        let tip = w[1].apply(Vec3::new(16.0, 10.0, 16.0));
        assert!((tip.x - 36.0).abs() < 1e-3 && (tip.y - 30.0).abs() < 1e-3, "{tip:?}");
    }

    #[test]
    fn walk_cycle_has_keys_for_existing_bones() {
        let mut m = VoxelModel::new("h", [64, 64, 64]);
        for name in ["hips", "upper_leg_l", "upper_leg_r"] {
            m.bones.push(Bone { name: name.into(), parent: None, head: Vec3::ZERO, tail: Vec3::ZERO });
        }
        let a = locomotion_cycle(&m, "walk", &CycleParams::walk(8));
        assert_eq!(a.frames, 8);
        assert_eq!(a.keys.len(), 3 * 8);
        let l = a.sample(1, 2.0).0.x;
        let r = a.sample(2, 2.0).0.x;
        assert!((l + r).abs() < 1e-3 && l.abs() > 10.0, "legs swing in opposition");
    }
}
