# Mimari

## Katmanlar

| Katman | Konum | Dil | Sorumluluk |
| --- | --- | --- | --- |
| Rust Core (`pixcore`) | `core-rust/` | Rust (`std`) | Belgenin tek otoritesi: pikseller, katmanlar, kareler, geçmiş, render, dışa aktarma, depolama |
| Native köprü | `core-rust/src/ffi`, `app-kotlin/shared/.../bridge` | Rust + Kotlin | El yazımı JNI (yalnızca 4 JNIEnv fonksiyonu) |
| Paylaşılan Kotlin | `app-kotlin/shared` | Kotlin/JVM | JSON, `PixelEngine` sarmalayıcısı |
| Agent Gateway | `agent-gateway` | Kotlin/JVM | HTTP sunucusu, MCP, REST, izinler, araç kataloğu |
| STDIO adaptörü | `agent-gateway/stdio-adapter` | Kotlin/JVM | Alt süreç MCP istemcileri için HTTP köprüsü |
| Windows/masaüstü | `app-kotlin/windows` | Kotlin + Swing | İnsan editörü |
| Android | `app-kotlin/android` | Kotlin + platform View'ları | İnsan editörü + Agent Mode servisi |
| Protokol | `protocol/tools` | JSON | MCP/OpenAI/Anthropic/Gemini araç şemaları (Kotlin kataloğundan üretilir) |

Gateway ve paylaşılan modüller saf JVM kodudur (`java.net`, `java.util`, `java.security`), bu yüzden hem Android'de hem masaüstünde değişmeden çalışır.

## Tek giriş noktası: komut protokolü

İnsan arayüzü, MCP, REST ve testlerin hepsi aynı çağrıyı yapar:

```json
{"op":"set_pixel","actor":"mcp:Claude Code","operationId":"op_1042","expectedRevision":125,
 "args":{"x":12,"y":18,"color":"#D94B42","frame":4}}
```

Yanıt `{"ok":true,"op":"set_pixel","revision":126,"actor":"...","result":{...}}` ya da `{"ok":false,...,"error":{"code":"conflict","message":"..."}}` biçimindedir. JNI çerçevesi `u32 LE json uzunluğu | JSON | ikili veri` şeklindedir; PNG, GIF, `.pxs` ve ham RGBA bu ikili kısımda taşınır.

İşlem listesi `core-rust/src/commands/mod.rs` içindeki `OPERATIONS` tablosundadır ve `status` sorgusuyla da alınabilir. Gateway testleri her aracın motorda var olan bir işleme karşılık geldiğini doğrular.

## Veri modeli

```
Document
 ├─ name, width, height (1..4096)
 ├─ palette: Vec<RGBA>  (renk örnekleri; tuval gerçek renk)
 ├─ models: [VoxelModel{size ≤ 512³, chunks 16³, bones, animations}]
 ├─ materials: [Material{PBR + 7 harita}], textures: [RGBA8]
 ├─ layers: [Layer{id, name, visible, locked, opacity}]   alttan üste
 ├─ frames: [Frame{id, duration_ms}]                       sabit kare sınırı yok
 ├─ tags:   [Tag{name, from_id, to_id, direction, repeat}] Idle/Walk/Attack
 └─ cels:   (layer_id, frame_id) → Arc<PixelGrid>
               PixelGrid = 16×16 tile ızgarası, tile = Option<Arc<[u16; 256]>>
```

- Pikseller **gerçek renk** (`0xRRGGBBAA`, u32) olarak tutulur; renk sınırı yoktur. Palet yalnızca renk örneği (swatch) listesidir. Bir örneği değiştirmek, eski rengi kullanan tüm pikselleri yeniden renklendirir.
- 3D modeller (`models`), PBR materyaller ve dokular aynı belgede bulunur. Ayrıntılar için [VOXEL.md](VOXEL.md).
- Tile'lar ve cel'ler **copy-on-write**'tır. Kare çoğaltmak işaretçi kopyalar; değişmeyen tile'lar kareler arasında fiziksel olarak paylaşılır. Undo anlık görüntüsü bu yüzden ucuzdur.
- Katman ve kare kimlikleri yeniden sıralamadan etkilenmez. API'de kullanıcılar indekslerle çalışır.

## Eşzamanlılık: insan + AI

- **Tek yazıcı:** FFI katmanı her motoru bir mutex arkasında tutar ve komutlar sırayla uygulanır.
- **İyimser revizyon kontrolü:** Her değişiklik revizyonu artırır. `expectedRevision` güncel değilse komut `conflict` ile reddedilir, böylece AI eski bir görüntüye dayanarak insanın yeni çizimini ezemez.
- **Idempotency:** Aynı `operationId` ikinci kez uygulanmaz; önbellekteki yanıt `replayed:true` ile döner.
- **Transaction:** `transaction` içindeki tüm komutlar ya birlikte uygulanır ya da hiçbiri uygulanmaz. Sonuç tek undo adımı ve tek revizyondur.
- **Geçmiş:** Her değişiklik `actor`, `op`, `operationId` ve değişen piksel sayısıyla kaydedilir (`history_list`, `.pxs` içindeki `HIST` bölümü). Undo, değişikliği kimin yaptığından bağımsız olarak çalışır.
- **Arayüz senkronu:** `PixelEngine` her revizyon değişiminde dinleyicileri tetikler; masaüstü ve Android arayüzü ajanın yaptığı değişiklikleri anında gösterir.

## Render

Render tamamen tamsayı aritmetiğiyle yapılır (`renderer::blend_over`) ve aynı girdi her platformda bit düzeyinde aynı sonucu verir. `tests/golden/scene.jsonl` + `scene.fingerprint` hem Rust testinde hem JVM/JNI testinde doğrulanır.

## Güvenlik

- Gateway varsayılan olarak yalnızca `127.0.0.1`'e bağlanır. 256 bit rastgele Bearer token zorunludur ve sabit zamanlı karşılaştırılır.
- DNS rebinding'e karşı `Origin` ve (loopback modunda) `Host` doğrulanır; geçersiz Origin `403` döner.
- Ajanlar yalnızca Pixel Studio araçlarına erişir; kabuk, dosya sistemi veya ağ aracı yoktur.
- Yıkıcı araçlar (`frame_delete`, `layer_delete`, `project_new`, `canvas_resize`) ve bunları içeren transaction'lar insan onayı ister. Arayüzsüz modda varsayılan cevap **ret**'tir.
- Dosya yazımı yalnızca çalışma alanı klasörüne yapılır ve dosya adları temizlenir (`../../etc/passwd` → `etc_passwd`).
- Rust tarafında bozuk JSON, aşırı büyük bölge, sınır dışı koordinat ve bozuk `.pxs` girdileri panik yerine hata döndürür; JVM'e asla panik geçmez.
