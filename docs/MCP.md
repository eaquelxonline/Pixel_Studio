# MCP bağlantısı

Endpoint: `http://127.0.0.1:38471/mcp` (port doluysa uygulama boş bir port seçer ve bağlantı penceresinde gösterir). Tüm istekler `Authorization: Bearer <token>` başlığı ister.

## Bağlanma

**Claude Code (HTTP):**
```bash
claude mcp add --transport http pixel-studio http://127.0.0.1:38471/mcp --header "Authorization: Bearer <TOKEN>"
```

**Antigravity / JSON yapılandırması:**
```json
{"mcpServers":{"pixel-studio":{"serverUrl":"http://127.0.0.1:38471/mcp","headers":{"Authorization":"Bearer <TOKEN>"}}}}
```

**STDIO isteyen istemciler:** `pixstudio-mcp` adaptörü (`./gradlew :agent-gateway:stdio-adapter:installDist`). Çizim motoru taşımaz; açık uygulamanın HTTP endpoint'ine köprü kurar.
```json
{"mcpServers":{"pixel-studio":{"command":"pixstudio-mcp","args":["--url","http://127.0.0.1:38471/mcp","--token","<TOKEN>"]}}}
```

**Android telefondaki Pixel Studio + bilgisayardaki ajan:** Telefonda Agent Mode'u açın, ardından USB üzerinden `adb forward tcp:38471 tcp:38471` çalıştırın. Bundan sonra bilgisayardaki ajan `127.0.0.1:38471` adresine bağlanır. Sunucu ağa açılmaz.

## Protokol sürümleri

Sunucu "dual-era"dır ve iki dönemi aynı endpoint'te sunar:

| İstemci | Davranış |
| --- | --- |
| `initialize` gönderen eski istemci (2024-11-05, 2025-03-26, 2025-06-18, 2025-11-25) | El sıkışma yapılır, desteklenen sürüm geri döner, `Mcp-Session-Id` verilir (yalnızca istemci adını geçmişe yazmak için) |
| 2026-07-28 istemcisi | Durumsuz çalışır. Her istek `params._meta["io.modelcontextprotocol/protocolVersion"]` ve `clientCapabilities` taşır. `MCP-Protocol-Version`, `Mcp-Method` ve gerekiyorsa `Mcp-Name` başlıkları doğrulanır (uyuşmazsa 400 / -32020). Desteklenmeyen sürüm 400 / -32022 döner. Sonuçlarda `resultType:"complete"` ve `_meta.serverInfo` bulunur. Liste yanıtlarında `ttlMs`/`cacheScope` vardır. `server/discover` desteklenir |

Yanıtlar her zaman `application/json` olarak döner. Bildirimler `202` alır. `GET /mcp` isteği `405` döner.

## Araçlar (104)

Tam şemalar `protocol/tools/pixel-studio-tools.mcp.json` dosyasındadır.

| Grup | Araçlar |
| --- | --- |
| Tuval | `canvas_inspect`, `canvas_snapshot` (PNG + özet), `get_pixel`, `read_region` (ascii/hex), `set_pixel`, `set_pixels`, `erase_pixel`, `draw_line`, `draw_rectangle`, `draw_ellipse`, `fill_region`, `replace_color`, `flip_horizontal`, `flip_vertical`, `rotate`, `move_region`, `copy_region`, `clear`, `canvas_resize`* |
| Animasyon | `frame_add`, `frame_duplicate`, `frame_delete`*, `frame_move`, `frame_get`, `frame_set_duration`, `frame_select`, `animation_tag_create`, `animation_tag_delete`, `animation_set_loop`, `animation_info`, `animation_preview` (GIF) |
| Katman | `layer_create`, `layer_delete`*, `layer_rename`, `layer_visibility`, `layer_lock`, `layer_opacity`, `layer_move`, `layer_select` |
| Palet | `palette_get`, `palette_set`, `palette_add` |
| Piksel art | `draw_ascii`, `outline`, `color_ramp`, `palette_list`, `palette_load`, `quantize`, `dither_fill`, `export_normal_map` |
| 3D Voxel | `voxel_model_create`, `voxel_model_clone`, `voxel_model_delete`*, `voxel_model_set`, `voxel_inspect`, `voxel_get`, `voxel_set`, `voxel_set_many`, `voxel_shape`, `voxel_mirror`, `material_set`, `texture_import`, `texture_generate`, `rig_bone_add`, `rig_clear`*, `rig_bind`, `anim3d_create`, `anim3d_key`, `anim3d_generate`, `voxel_render` (PNG), `voxel_render_sheet` (PNG), `voxel_render_to_frames`, `voxel_pick` |
| Oyun üretimi | `layer_blend`, `slice_set`, `slice_delete`, `palette_swap`, `rotsprite`, `tileset_create`, `tileset_add_tiles`, `tileset_generate_blob`, `tileset_delete`*, `tilemap_create`, `tilemap_delete`*, `tilemap_set`, `tilemap_terrain`, `tilemap_bake`, `tilemap_render` (PNG), `export_tileset` (PNG), `render_lit` (PNG), `export_atlas`, `export_godot_spriteframes`, `export_tiled`, `gamedev_inspect` — bkz. [GAMEDEV.md](GAMEDEV.md) |
| Proje | `image_import`, `project_inspect`, `project_save`, `project_export` (png/gif/spritesheet/atlas/godot/tiled/normal_map), `project_new`*, `project_rename`, `history_undo`, `history_redo`, `history_list`, `transaction` |

`*` işaretli araçlar insan onayı ister. Değişiklik yapan her araç isteğe bağlı `expectedRevision` ve `operationId` parametrelerini kabul eder.

**AI'nın gözleri:** `canvas_snapshot` görsel destekleyen modeller için büyütülmüş bir PNG döndürür; ayrıca içerik sınırlarını ve kullanılan renkleri JSON olarak verir. `read_region` görsel desteklemeyen modeller için pikselleri metin olarak sunar:

```
"legend": {".":"transparent","0":"#F6D7B0","1":"#000000"},
"rows": ["..000..", ".01010.", "..000.."]
```

## Kaynaklar ve promptlar

- Kaynaklar: `pixelstudio://project/current`, `pixelstudio://project/palette`, `pixelstudio://frame/current` (PNG), `pixelstudio://animation/current`, `pixelstudio://guide/pixel-art`
- Promptlar: `draw_sprite`, `animate_sprite`, `build_level`, `review_canvas`
- Sunucu `instructions` alanı ajanlara iş akışını öğretir: önce incele, palet ve katman planla, bloklarla başla, `set_pixels` ile detaylandır, `canvas_snapshot` ile kontrol et, kareleri çoğalt ve yalnızca değişeni düzenle.
