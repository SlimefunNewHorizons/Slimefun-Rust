# Motor nativo Rust: estado real (2026-10-28)

## Que existe y que se usa de verdad
- `slimefun-ffi` (cdylib, JNI + C ABI): ABI 2. Lo unico invocado desde Java en produccion:
  `sumSaturating` y `calculateMarketPrice` (via `RustNativeEngine` en Slimefun4-Drake, DrakesSlimeMarket).
- `slimefun-energy`: solver de grafo con DashMap, **nadie le registra nodos desde Java**; `fill_sequential`/`store_remaining` (ABI 2) son equivalentes exactos del bucle de EnergyNet, probados, **sin cablear**.
- `slimefun-cargo` (stub: devuelve `amount`), `slimefun-addons` (stubs), `TickerEngine` (solo cuenta ticks): sin logica real.
- `RustNativeBridge` de los addons: solo hace `System.load`, no llama nada.
- DrakesPurpur `SlimefunRustEngine` (Panama): antes simulaba resultados (`wasted=150`) y afirmaba "0 MB"; ahora no simula y verifica ABI.

## Medido (bench/run.sh, JDK 21, G1, ver docs/bench_2026-10-28.txt)
Rust via JNI **no gana** en ningun tamano (n=2..65536): sum 16 elem: Java 18 ns vs JNI 65 ns; fill 1024: 1455 vs 1525 ns; 65536: 91 vs 94 us. El costo real de EnergyNet es getCharge/setCharge (BlockStorage/Bukkit), que no puede salir de Java.
Lo que si reduce presion de GC: quitar arrays temporales. Patron anterior: 24 B (n=2) a 262 KB (n=65536) por tick de regulador; nuevo: 0 B y ~25-35 % menos tiempo.

## Decisiones
- EnergyNet usa `NativeAccelerationService.addSaturating` (Java, sin alloc), por defecto.
- `native-engine.native-sum: false` por defecto.
- El GC no se elimina; solo baja la asignacion en ese camino.

## Pendiente
- Off-heap SoA real (datos viven en Rust, sin gather/scatter) para que `fill_sequential` compense: requiere cambiar el modelo de EnergyNet; no iniciado.
- Ruteo de cargo, indice espacial de ticker: sin implementar.
- Medir en servidor real con JFR (`jcmd JFR.start`) la tasa de asignacion antes/despues; Panama no medido (JDK 25).
