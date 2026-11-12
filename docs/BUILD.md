# Derleme

## Araç zinciri (doğrulanan sürümler)

| Araç | Sürüm |
| --- | --- |
| Rust | 1.100.0 (edition 2024, `rust-toolchain.toml` ile sabit), yalnızca `std` |
| JDK | 25 LTS (bytecode hedefi 17) |
| Kotlin | 2.4.21 |
| Gradle | 9.9.1 (wrapper) |
| Android Gradle Plugin | 9.5.1 (yerleşik Kotlin) |
| Android | compileSdk 37.2, targetSdk 37 (Android 17), minSdk 30 (Android 11) |
| NDK | 30.0.16248370 |

## Rust çekirdeği

```bash
cd core-rust
cargo test                    # 55 test (golden render dahil)
cargo clippy --all-targets    # uyarısız
cargo run --release --example run_script -- ../tests/golden/scene.jsonl out/ 32 32 10   # betiği PNG/GIF/.pxs'e dönüştürür
cargo run --release --example run_script -- ../assets/knight/knight_showcase.jsonl out/  # şövalye + sprite'lar
```

## Masaüstü / Windows

```bash
./gradlew :app-kotlin:windows:run          # host platformunda çalıştırır
```

Windows x64 kurulum paketi (Windows makinesinde):

```powershell
cd core-rust; cargo build --release --target x86_64-pc-windows-msvc; cd ..
.\gradlew :app-kotlin:windows:packageWindows                       # .exe kurulum
.\gradlew :app-kotlin:windows:packageWindows -PpackageType=app-image   # taşınabilir klasör
```

`jpackage` JRE'yi pakete gömer, `pixcore.dll` dosyasını uygulama klasörüne koyar, `.pxs` dosya ilişkilendirmesini kaydeder ve `branding/pixel-studio.ico` simgesini kullanır.

## Android

`local.properties` içine `sdk.dir=/path/to/android-sdk` yazın ya da `ANDROID_HOME` tanımlayın. Android modülü yalnızca SDK bulunduğunda projeye dahil edilir.

```bash
rustup target add aarch64-linux-android x86_64-linux-android
./gradlew :app-kotlin:android:assembleDebug
```

`cargoBuild-arm64-v8a` ve `cargoBuild-x86_64` görevleri çekirdeği NDK'nın clang'ıyla (API 30) derler ve `libpixcore.so` dosyasını APK'ya koyar. Çıktı 16 KB sayfa hizalıdır.

## Arayüzsüz sunucu

```bash
./gradlew :agent-gateway:run --args="--size 64x64 --port 38471 --workspace ./workspace [--token T] [--allow-destructive]"
```

## Testler

| Komut | Kapsam |
| --- | --- |
| `cargo test` (core-rust) | JSON, raster, render, PNG/GIF/zlib, .pxs, motor, FFI, golden |
| `./gradlew :app-kotlin:shared:test` | Kotlin JSON, JNI yaşam döngüsü, çakışma, golden eşleşmesi, ImageIO ile PNG/GIF çözme |
| `./gradlew :agent-gateway:test` | HTTP güvenliği, eski ve 2026 MCP, insan+AI çakışması, onay akışı, REST, function calling, çalışma alanı sınırı, STDIO adaptörü, şema güncelliği |
