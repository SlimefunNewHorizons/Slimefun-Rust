<div align="center">

  <img src="https://raw.githubusercontent.com/DrakesCraft-Labs/Slimefun-Rust/main/slimefun_rust_banner.svg" alt="Slimefun-Rust Banner" width="920" />

# Slimefun-Rust Engine

**Native acceleration core for Slimefun and the DrakesCraft plugin ecosystem.**

<p>
  <a href="https://github.com/DrakesCraft-Labs/Slimefun-Rust"><img src="https://img.shields.io/badge/GitHub-Slimefun--Rust-181717?style=for-the-badge&logo=github" alt="GitHub"/></a>
  <img src="https://img.shields.io/badge/Rust-2021_Workspace-FF4500?style=for-the-badge&logo=rust&logoColor=white" alt="Rust 2021"/>
  <img src="https://img.shields.io/badge/Java-21_FFM_Panama-F89820?style=for-the-badge&logo=openjdk&logoColor=white" alt="Java 21 FFM"/>
  <img src="https://img.shields.io/badge/Purpur-1.21.11-00FF66?style=for-the-badge&logo=minecraft&logoColor=white" alt="Purpur 1.21.11"/>
  <img src="https://img.shields.io/badge/Addons-44_Integrated-00F2FE?style=for-the-badge" alt="44 Addons"/>
</p>

</div>

> ### 🏰 ¡Únete a la Comunidad Oficial de DrakesCraft!
> 
> * 🎮 **IP del Servidor**: `mc.drakescraft.cl` *(Java 1.21.11 & Bedrock)*
> * 💬 **Discord Oficial**: [discord.gg/drakescraft](https://discord.gg/rv3vtXZTk7)
> * 🌐 **Web & Guía**: [web.drakescraft.cl](https://web.drakescraft.cl) — 🛒 **Tienda**: [web.drakescraft.cl/store](https://web.drakescraft.cl/store.html)
> 
> *¡Juega con este addon y más de 80 expansiones optimizadas en vivo en nuestra network de supervivencia técnica!*

---

---

## Status

This repository is the **strategic native core** intended to accelerate shared
work across Slimefun4-Drake, its addons and DrakesCraft-owned plugins. It is not
retired.

ABI 1 provides a tested Linux JNI bridge consumed by Slimefun4-Drake. EnergyNet
uses native saturating aggregation and DrakesSlimeMarket delegates deterministic
price calculation through the shared Bukkit service. Java remains authoritative
for Bukkit state and provides the automatic fallback.

## Runtime contract

- Bukkit/Paper lifecycle and all world mutations remain on the server thread.
- Java publishes immutable work snapshots to Rust and applies validated results.
- Rust must never mutate live Bukkit objects from native worker threads.
- Java remains the fallback when native loading or a native calculation fails.
- Persistent native writes remain behind a separate migration gate and verified
  backups.
- Pterodactyl runs Linux, so production requires `libslimefun_ffi.so`.
  `slimefun_ffi.dll` is only a Windows build and must not be placed in the live
  Linux plugin directory.

The addon table below is the target integration catalog. Entries marked by the
registry are not proof that every addon already delegates production work to
Rust.

## ABI 1

- `nativeAbiVersion`: bridge compatibility check.
- `nativeSumSaturating`: EnergyNet aggregation with Java-equivalent overflow.
- `nativeCalculateMarketPrice`: bounded market pricing for DrakesSlimeMarket.
- `/sf native`: live availability, native calls, fallbacks and failures.

### El nombre del paquete Java es parte del contrato — no renombrar

Los simbolos que exporta `slimefun-ffi` se derivan del paquete de la clase Java, porque asi
resuelve JNI los metodos `native`:

```
Java_io_github_thebusybiscuit_slimefun4_core_services_nativeengine_RustNativeEngine_nativeAbiVersion
     └────────────── paquete Java ──────────────┘└──── clase ────┘└─── metodo ───┘
```

`RustNativeEngine` vive en `io.github.thebusybiscuit...` y **no** en el namespace de DrakesCraft,
que es donde esta el resto del fork (679 ficheros contra 8). Parece una incoherencia del porteo
y no lo es: ese arbol es la capa de compatibilidad que conservan los addons de terceros, y
`NativeAccelerationService` forma parte de ella porque **DrakesSlimeMarket la importa** para el
calculo de precios.

Mover la clase de paquete rompe el motor en silencio: los simbolos del `.so` dejan de coincidir,
`System.load` funciona pero `nativeAbiVersion()` lanza `UnsatisfiedLinkError`, y Slimefun cae al
fallback de Java sin que nadie lo note salvo por `/sf native`.

Si algun dia hay que renombrarlo, hay que hacer las tres cosas a la vez y en el mismo reinicio:

1. Cambiar el paquete en `RustNativeEngine.java` y `NativeAccelerationService.java`.
2. Renombrar las tres funciones `Java_...` en `crates/slimefun-ffi/src/`.
3. Recompilar el `.so`, recompilar Slimefun4-Drake **y** recompilar DrakesSlimeMarket.

Verificacion antes de desplegar:

```bash
cargo build --release -p slimefun-ffi
nm -D --defined-only target/release/libslimefun_ffi.so | grep -oE 'Java_[A-Za-z0-9_]+'
```

Los tres simbolos que salgan tienen que coincidir exactamente con los metodos `native` que
declara `RustNativeEngine.java`. Si no coinciden, no desplegar.

### Estado verificado en produccion (2026-08-20)

```
Slimefun-Rust | Estado: ACTIVO
ABI: 1 | Llamadas nativas: 171216 | Fallbacks: 79548 | Fallos: 0
```

Los fallbacks **no son errores**: son lotes por debajo de `native-engine.minimum-batch-size`
(2 por defecto), donde cruzar a nativo cuesta mas de lo que ahorra. Cero fallos en 171.216
llamadas es la señal de que el puente esta sano.

El motor solo escribe una linea al arrancar --"Motor JNI activo (ABI 1)"-- asi que buscarlo en
un log ya rotado da la impresion falsa de que no carga. **La comprobacion buena es `/sf native`**,
no el log.

---

## 🏛️ Estructura del Monorepo

```
Slimefun-Rust/
 ├── Cargo.toml                       <-- Workspace Configuration
 ├── slimefun_rust_banner.svg         <-- Animated SVG Banner
 └── crates/
      ├── slimefun-core/              <-- BlockStorage (SQLite/JSON), Ítems y Ticker Paralelo
      ├── slimefun-energy/            <-- EnergyNet Graph Solver (Rayon + Petgraph)
      ├── slimefun-cargo/             <-- CargoNet Item Router & Node Logistics
      ├── slimefun-addons/            <-- Motor unificado para los 44 Addons de Slimefun
      ├── slimefun-ffi/               <-- C-ABI Shared Library (.dll / .so) para Java 21 Panama FFM
      └── slimefun-server/            <-- Microservicio Standalone en Axum/Tokio (Puerto 8085)
```

---

## Storage integration

Until schema detection, migrations and crash recovery are covered by tests,
validate this API against copied fixtures rather than the live
`stored-blocks.db`.

El crate `slimefun-core` implementa una lectura/escritura bidireccional sobre la base de datos SQLite `stored-blocks.db` nativa de Slimefun4:

```rust
use slimefun_core::BlockStorageEngine;

let storage = BlockStorageEngine::new();
// Carga transaccional directa de stored-blocks.db
let total_loaded = storage.load_sqlite_db("data-storage/Slimefun/stored-blocks.db")?;
println!("Bloques Slimefun cargados en memoria nativa: {}", total_loaded);
```

---

## 📋 Target Addon Catalog

| # | Addon de Slimefun | Categoría | Módulo en Rust |
| :-: | :--- | :--- | :--- |
| **1** | **NetworksV6-Drake** | `CARGO_ENERGY` | `networks.rs` |
| **2** | **FluffyMachines** | `AUTOMATION` | `fluffy_machines.rs` |
| **3** | **Supreme-Drake** | `ADVANCED_TECH` | `supreme.rs` |
| **4** | **DynaTech** | `AUTOMATION` | `dynatech.rs` |
| **5** | **InfinityExpansion** | `ENDGAME` | `infinity_expansion.rs` |
| **6** | **AdvancedTech** | `MACHINERY` | `all_addons.rs` |
| **7** | **AlchimiaVitae** | `MAGIC` | `all_addons.rs` |
| **8** | **BetterChests** | `STORAGE` | `all_addons.rs` |
| **9** | **ChestTerminal** | `STORAGE` | `all_addons.rs` |
| **10** | **ColoredEnderChests** | `STORAGE` | `all_addons.rs` |
| **11** | **Cultivation** | `AGRICULTURE` | `all_addons.rs` |
| **12** | **DankTech2** | `STORAGE` | `all_addons.rs` |
| **13** | **DyeBench** | `CRAFTING` | `all_addons.rs` |
| **14** | **DyedBackpacks** | `STORAGE` | `all_addons.rs` |
| **15** | **EcoPower** | `ENERGY` | `all_addons.rs` |
| **16** | **ElectricSpawners** | `MOBS` | `all_addons.rs` |
| **17** | **ElementManipulation** | `MAGIC` | `all_addons.rs` |
| **18** | **ExoticGarden** | `AGRICULTURE` | `all_addons.rs` |
| **19** | **ExtraGear** | `WEAPONS_ARMOR` | `all_addons.rs` |
| **20** | **ExtraTools** | `TOOLS` | `all_addons.rs` |
| **21** | **FastMachines** | `AUTOMATION` | `all_addons.rs` |
| **22** | **FlowerPower** | `AGRICULTURE` | `all_addons.rs` |
| **23** | **FNAmplifications** | `MACHINERY` | `all_addons.rs` |
| **24** | **FoxyMachines** | `AUTOMATION` | `all_addons.rs` |
| **25** | **Galaxyfun** | `SPACE_ENDGAME` | `all_addons.rs` |
| **26** | **GeneticChickengineering** | `MOBS` | `all_addons.rs` |
| **27** | **HotbarPets** | `ITEMS` | `all_addons.rs` |
| **28** | **LiteXpansion** | `MACHINERY` | `all_addons.rs` |
| **29** | **MobCapturer** | `MOBS` | `all_addons.rs` |
| **30** | **MoreResearches** | `RESEARCH` | `all_addons.rs` |
| **31** | **PotionExpansion** | `MAGIC` | `all_addons.rs` |
| **32** | **RelicsOfCthonia** | `MAGIC_WEAPONS` | `all_addons.rs` |
| **33** | **SensibleToolbox** | `AUTOMATION` | `all_addons.rs` |
| **34** | **SFCalc** | `UTILITY` | `all_addons.rs` |
| **35** | **SimpleMaterialGenerators** | `GENERATORS` | `all_addons.rs` |
| **36** | **SlimeFrame** | `UTILITY` | `all_addons.rs` |
| **37** | **SlimefunLuckyBlocks** | `FUN` | `all_addons.rs` |
| **38** | **SlimeHUD** | `UI` | `all_addons.rs` |
| **39** | **SlimeTinker** | `WEAPONS` | `all_addons.rs` |
| **40** | **SlimyTreeTaps** | `AGRICULTURE` | `all_addons.rs` |
| **41** | **SoulJars** | `MOBS` | `all_addons.rs` |
| **42** | **TranscEndence** | `ENDGAME` | `all_addons.rs` |
| **43** | **WorldEditSlimefun** | `WORLD_BUILDING` | `all_addons.rs` |
| **44** | **DrakesSlimeMarket** | `ECONOMY` | `all_addons.rs` |

---

## 🛠️ Compilación y Construcción

```bash
# Comprobar la compilación de todos los crates del Monorepo
cargo check --workspace

# Compilar binarios de producción (slimefun-server.exe y slimefun_ffi.dll)
cargo build --release --workspace
```

---

<div align="center">

**DrakesCraft Labs** · Desarrollado por [**JackStar6677-1**](https://github.com/JackStar6677-1)

</div>

---

## 📄 License & Intellectual Property

Copyright © 2026 [**JackStar6677-1**](https://github.com/JackStar6677-1) · [**DrakesCraft Labs**](https://github.com/DrakesCraft-Labs). All Rights Reserved.

This software is **Source-Available** for public inspection and technical audit. Redistribution, commercial repackaging, or unauthorized derivative distribution without explicit written permission from the author is strictly prohibited.
