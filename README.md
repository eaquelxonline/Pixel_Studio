# Pixel Studio

**com.pixel.studio** — insanların ve yapay zekâ ajanlarının aynı piksel çalışma alanında, gerçek piksel araçlarıyla birlikte çizim ve animasyon yaptığı bağımsız sanat platformu.

Pixel Studio'nun kendi yapay zekâsı yoktur. Zekâyı dışarıdan gelen ajan sağlar (Claude Code, Antigravity, ChatGPT, Gemini ya da kendi ajanınız). Pixel Studio ise onun **elleri, gözleri ve çalışma masası** olur: `set_pixel`, `draw_line`, `frame_duplicate` gibi gerçek editör araçları, revizyon kontrolü, geri alma ve izin sistemiyle birlikte.

```
AI / İnsan ──► MCP (/mcp) · REST (/api/v1) · Function calling · STDIO
                         │
              Kotlin (Android Views / Windows Swing) ── JNI ──► Rust pixcore
                                                               piksel · katman · kare · geçmiş
                                                               render · PNG/GIF · .pxs
```

## Durum — Foundation Sprint

| Parça | Durum | Doğrulama |
| --- | --- | --- |
| Rust çekirdek (`core-rust`, yalnızca `std`) | ✅ | 37 test, clippy temiz |
| Piksel/palet, 16×16 COW tile, katman, dinamik kare, etiket, onion skin | ✅ | birim testleri |
| Revizyon kontrolü, `operationId` idempotency, atomik transaction, undo/redo | ✅ | motor testleri |
| PNG (kendi zlib'imiz), GIF (kendi LZW'miz), sprite sheet + JSON | ✅ | PIL ve JDK ImageIO ile çözüldü |
| `.pxs` ikili format (tile tekilleştirme, RLE, CRC-32) | ✅ | gidiş-dönüş + bozulma testleri |
| El yazımı JNI köprüsü | ✅ | JVM üzerinde gerçek çağrılar |
| Golden test: Rust ↔ JVM bit düzeyinde aynı render | ✅ | `tests/golden` |
| MCP sunucusu: eski `initialize` (2024-11-05 … 2025-11-25) **ve** 2026-07-28 (`_meta`, `server/discover`) | ✅ | uçtan uca HTTP testleri |
| REST API + OpenAI/Anthropic/Gemini araç şemaları | ✅ | uçtan uca testler |
| STDIO adaptörü (`pixstudio-mcp`) | ✅ | uçtan uca test |
| Windows/masaüstü editörü (Swing) | ✅ | derleniyor; Xvfb altında çalıştırıldı |
| Android editörü (platform View'ları) + Agent Mode servisi | 🟡 | APK derleniyor, lint temiz; **cihazda/emülatörde henüz çalıştırılmadı** |
| LAN + TLS eşleştirme, bulut relay | ⏳ | Aşama 7 (şimdilik `adb forward` ile USB) |

## Hızlı başlangıç

```bash
# Rust testleri
cd core-rust && cargo test

# Tüm Kotlin testleri (Rust çekirdeğini otomatik derler)
./gradlew test

# Masaüstü editörü (Linux/macOS/Windows)
./gradlew :app-kotlin:windows:run

# Arayüzsüz sunucu (CI veya sunucu kullanımı)
./gradlew :agent-gateway:run --args="--size 64x64 --workspace ./workspace"
```

Uygulama açıldığında **Agent → Connect an agent…** penceresi bağlantı komutlarını ve token'ı gösterir:

```bash
claude mcp add --transport http pixel-studio http://127.0.0.1:38471/mcp --header "Authorization: Bearer <TOKEN>"
```

Sonra ajana şunu söyleyin: *"Pixel Studio'yu kullan. 64x64 bir samuray çiz, ardından 24 karelik koşu animasyonu yap. Her kareyi kendin çiz ve tutarlılığı kontrol et."*

## Dokümanlar

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): katmanlar, veri modeli, eşzamanlılık
- [docs/MCP.md](docs/MCP.md): MCP bağlantısı, araçlar, kaynaklar, promptlar, protokol sürümleri
- [docs/API.md](docs/API.md): REST API ve function calling
- [docs/PROJECT-FORMAT.md](docs/PROJECT-FORMAT.md): `.pxs` dosya formatı
- [docs/BUILD.md](docs/BUILD.md): araç zinciri, Android/Windows derleme ve paketleme

## Bağımlılık politikası

Çalışma zamanında üçüncü taraf kütüphane yoktur. Rust tarafı yalnızca `std` kullanır. Kotlin tarafında tek çalışma zamanı bağımlılığı dilin kendi `kotlin-stdlib`'idir; UI için platform API'leri kullanılır (Android `View`/`Canvas`, JDK Swing). Derleme araçları (Gradle, AGP, NDK, JDK) ve yalnızca test kapsamındaki `kotlin-test` bu politikanın dışındadır.
