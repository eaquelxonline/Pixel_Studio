# Universal Pixel API (REST)

Taban adres: `http://127.0.0.1:38471/api/v1`. Tüm istekler `Authorization: Bearer <token>` başlığı ister. İsteğe bağlı `X-Pixel-Actor: <ad>` başlığı geçmişe `api:<ad>` olarak yazılır.

| Metot ve yol | İşlev |
| --- | --- |
| `GET /status` | Motor durumu, revizyon, işlem listesi, gateway bilgisi |
| `GET /project` | `project_inspect` |
| `GET /canvas` | `canvas_inspect` |
| `GET /frame/{i}` | Kare bilgisi |
| `GET /frame/{i}/pixels?x=&y=&width=&height=&format=ascii\|hex\|indexed` | Piksel okuma |
| `GET /snapshot?frame=&scale=` | PNG (`image/png`, `X-Pixel-Revision` başlığıyla) |
| `GET /tools?format=mcp\|openai\|anthropic\|gemini` | Function-calling araç şemaları |
| `POST /tools/{name}` | Bir aracı çağırır. Gövde, modelin ürettiği argüman nesnesidir |
| `POST /commands` | Ham motor komutu: tek nesne, dizi ya da `{"commands":[...]}` |
| `POST /frames` | `frame_add` |
| `POST /animations` | `animation_tag_create` |
| `POST /export` | `{"format":"png\|gif\|spritesheet\|pxs", ...}` → ikili dosya |

Durum kodları: `200` başarı, `400` geçersiz argüman, `401` token yok veya yanlış, `403` izin yok ya da Origin geçersiz, `404` bulunamadı, `409` revizyon çakışması.

## Function calling ile kullanım

1. `GET /api/v1/tools?format=openai` (ya da `anthropic`, `gemini`) ile araç şemalarını alın ve modele verin.
2. Model bir araç çağrısı ürettiğinde argümanları olduğu gibi `POST /api/v1/tools/<name>` adresine gönderin.
3. Yanıttaki `content` (metin/görsel) ve `structuredContent` alanlarını modele geri iletin.

```bash
curl -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
     -d '{"x":0,"y":0,"width":8,"height":8,"color":"#3366FF","filled":true}' \
     http://127.0.0.1:38471/api/v1/tools/draw_rectangle
```

Şemaların JSON Schema kaynağı Kotlin kataloğudur (`ToolCatalog.kt`). `./gradlew :agent-gateway:exportToolSchemas` komutu `protocol/tools/*.json` dosyalarını yeniden üretir; testler bu dosyaların güncel olup olmadığını kontrol eder.
