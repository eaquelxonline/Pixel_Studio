# `.pxs` proje formatı (sürüm 2)

Tüm tamsayılar little-endian'dır. Metinler `u32 uzunluk + UTF-8` olarak saklanır.

```
"PXS\0" | version u16 = 1 | reserved u16
chunk* : tag[4] | length u32 | payload
"END\0" | 4 | CRC-32 (END'den önceki tüm baytlar)
```

| Bölüm | İçerik |
| --- | --- |
| `META` | ad, genişlik, yükseklik, aktif katman id, aktif kare id, sonraki katman id, sonraki kare id |
| `PALT` | renk örnekleri (swatch): sayı + `u32 0xRRGGBBAA` (little-endian). Tuval gerçek renk olduğu için pikseller palete bağlı değildir |
| `LAYR` | katmanlar alttan üste: id, ad, görünür, kilitli, opaklık |
| `FRAM` | kareler sırayla: id, süre (ms) |
| `TAGS` | ad, başlangıç kare id, bitiş kare id, yön (0 ileri, 1 geri, 2 ping-pong), tekrar (0 = sonsuz) |
| `TILE` | tekilleştirilmiş 16×16 tile'lar, RLE: `run sayısı u16` + `(uzunluk u16, piksel u32 0xRRGGBBAA)*` |
| `CELS` | `(katman id, kare id)` + satır satır tile referansları (`0` = boş, `n` = TILE[n-1]) |
| `HIST` | işlem günlüğü: revizyon, aktör, işlem, operationId, değişen piksel, zaman damgası |
| `MATL` | PBR materyaller: ad, roughness, metallic, emission, uv_scale, normal_strength, 7 harita için doku indeksi (`-1` = yok) |
| `TXTR` | dokular: ad, genişlik, yükseklik, zlib ile sıkıştırılmış RGBA8 |
| `VOXM` | voksel modeller: ad, boyut, origin, kemikler (ad, parent, head, tail), animasyonlar (ad, kare sayısı, ms, döngü, anahtarlar), zlib ile sıkıştırılmış 16³ chunk'lar (voksel başına `rgba u32, materyal u8, kemik u8`) |

- Aynı içeriğe sahip tile'lar bir kez saklanır. Çoğaltılmış ya da bekletilmiş animasyon kareleri neredeyse hiç yer kaplamaz.
- Okuyucu tüm sayıları, kimlikleri, palet referanslarını ve RLE uzunluklarını doğrular. Bozuk ya da kesik dosya hata verir, panik oluşturmaz.
- Bilinmeyen bölümler atlanır (ileriye uyumluluk). Kaydetme deterministiktir: yükle → kaydet işlemi aynı baytları üretir.
