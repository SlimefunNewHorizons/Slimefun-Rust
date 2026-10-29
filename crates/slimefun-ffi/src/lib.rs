use jni::objects::{JClass, JIntArray, ReleaseMode};
use jni::sys::{jdouble, jint, jlong};
use jni::JNIEnv;
use slimefun_core::{BlockStorageEngine, TickerEngine};
use slimefun_energy::{
    fill_sequential, store_remaining, sum_saturating, EnergyNetGraphSolver, EnergyReport,
};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::{OnceLock, RwLock};

static STORAGE_ENGINE: OnceLock<BlockStorageEngine> = OnceLock::new();
static ENERGY_SOLVER: OnceLock<EnergyNetGraphSolver> = OnceLock::new();
static TICKER_ENGINE: OnceLock<TickerEngine> = OnceLock::new();
static ULTIMO_REPORTE: OnceLock<RwLock<EnergyReport>> = OnceLock::new();
/// ABI 1: sumSaturating + calculateMarketPrice. ABI 2: anade fillSequential/storeRemaining por
/// lotes, acceso a arrays sin copia (critical) y todos los puntos de entrada blindados contra
/// panics. Java acepta 1 (solo funciones v1) y 2.
const ABI_VERSION: i32 = 2;
/// Codigo devuelto cuando algo falla dentro de Rust (panic, array invalido, longitudes distintas).
const ERR: i32 = i32::MIN;

/// Ejecuta `f` sin dejar que un panic cruce la frontera FFI (UB). Devuelve `fallback` si panica.
fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(fallback)
}

fn get_storage() -> &'static BlockStorageEngine {
    STORAGE_ENGINE.get_or_init(BlockStorageEngine::new)
}

fn get_energy_solver() -> &'static EnergyNetGraphSolver {
    ENERGY_SOLVER.get_or_init(EnergyNetGraphSolver::new)
}

fn get_ticker() -> &'static TickerEngine {
    TICKER_ENGINE.get_or_init(TickerEngine::new)
}

fn ultimo_reporte() -> &'static RwLock<EnergyReport> {
    ULTIMO_REPORTE.get_or_init(|| RwLock::new(EnergyReport::default()))
}

/// Carga la base de datos SQLite actual de Slimefun (`stored-blocks.db`) sin resetear el servidor.
#[no_mangle]
pub extern "C" fn slimefun_load_sqlite_db(path_ptr: *const c_char) -> i64 {
    if path_ptr.is_null() {
        return -1;
    }
    let c_str = unsafe { CStr::from_ptr(path_ptr) };
    if let Ok(path_str) = c_str.to_str() {
        let storage = get_storage();
        if let Ok(count) = storage.load_sqlite_db(path_str) {
            return count as i64;
        }
    }
    -1
}

/// Guarda el estado actual en la base de datos SQLite de Slimefun (`stored-blocks.db`).
#[no_mangle]
pub extern "C" fn slimefun_save_sqlite_db(path_ptr: *const c_char) -> i64 {
    if path_ptr.is_null() {
        return -1;
    }
    let c_str = unsafe { CStr::from_ptr(path_ptr) };
    if let Ok(path_str) = c_str.to_str() {
        let storage = get_storage();
        if let Ok(count) = storage.save_sqlite_db(path_str) {
            return count as i64;
        }
    }
    -1
}

/// Resuelve el reparto de energía y avanza el tick. Devuelve el número de tick.
///
/// Ojo con lo que **no** hace: el tick todavía no ejecuta la lógica de las máquinas, solo lleva
/// la cuenta. Ver `TickerEngine::tick_all_machines`.
///
/// El resultado del reparto de energía se guarda para consultarlo aparte: antes se descartaba en
/// el acto, así que el déficit de una red no llegaba a ninguna parte.
#[no_mangle]
pub extern "C" fn slimefun_execute_tick_cycle() -> u64 {
    let ticker = get_ticker();
    let storage = get_storage();
    let solver = get_energy_solver();

    let energia = solver.solve_tick();
    if let Ok(mut guard) = ultimo_reporte().write() {
        *guard = energia;
    }

    ticker.tick_all_machines(storage).tick
}

/// Demanda que quedó sin cubrir en el último tick. 0 significa que la red va sobrada.
///
/// Es el dato que de verdad se quiere vigilar: una red hambrienta se nota como máquinas que se
/// paran sin motivo aparente, y hasta ahora no había forma de verlo desde Java.
#[no_mangle]
pub extern "C" fn slimefun_last_unmet_demand() -> u64 {
    ultimo_reporte().read().map(|r| r.unmet).unwrap_or(0)
}

/// Energía desperdiciada en el último tick por no caber en ningún capacitor.
#[no_mangle]
pub extern "C" fn slimefun_last_wasted_energy() -> u64 {
    ultimo_reporte().read().map(|r| r.wasted).unwrap_or(0)
}

/// Energía generada en el último tick.
#[no_mangle]
pub extern "C" fn slimefun_last_generated_energy() -> u64 {
    ultimo_reporte().read().map(|r| r.generated).unwrap_or(0)
}

/// Devuelve el número total de bloques registrados en BlockStorage.
#[no_mangle]
pub extern "C" fn slimefun_get_total_block_count() -> usize {
    get_storage().count()
}

/// Suma valores enteros manteniendo la misma saturación que EnergyNet en Java.
#[no_mangle]
pub extern "C" fn slimefun_sum_saturating(values: *const i32, length: usize) -> i32 {
    if values.is_null() || length == 0 {
        return 0;
    }

    guard(0, || {
        let slice = unsafe { std::slice::from_raw_parts(values, length) };
        sum_saturating(slice)
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_thebusybiscuit_slimefun4_core_services_nativeengine_RustNativeEngine_nativeAbiVersion(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    ABI_VERSION
}

/// Suma sin copiar ni asignar: lee el array de Java en sitio (critical, sin copia de vuelta).
fn jni_sum(env: &mut JNIEnv, values: &JIntArray) -> jint {
    guard(0, || {
        // SAFETY: dentro de la seccion critica solo se lee el slice; no se llama a JNI.
        match unsafe { env.get_array_elements_critical(values, ReleaseMode::NoCopyBack) } {
            Ok(elems) => sum_saturating(&elems),
            Err(_) => 0,
        }
    })
}

/// Reparto por lotes (ABI 2). `charge` se modifica en sitio; `out2[0]` = energia restante,
/// `out2[1]` = demanda. Devuelve 0 si fue bien o `ERR`. Sin asignaciones en el camino.
#[no_mangle]
pub extern "system" fn Java_io_github_thebusybiscuit_slimefun4_core_services_nativeengine_RustNativeEngine_nativeFillSequential(
    mut env: JNIEnv,
    _class: JClass,
    supply: jint,
    capacity: JIntArray,
    charge: JIntArray,
    out2: JIntArray,
) -> jint {
    guard(ERR, || {
        let (Ok(n), Ok(m), Ok(o)) = (
            env.get_array_length(&capacity),
            env.get_array_length(&charge),
            env.get_array_length(&out2),
        ) else {
            return ERR;
        };
        if n != m || o < 2 {
            return ERR;
        }
        // SAFETY: tres arrays distintos de Java, sin llamadas JNI dentro de las secciones.
        let (rest, dem) = unsafe {
            let mut env2 = env.unsafe_clone();
            let Ok(cap) = env.get_array_elements_critical(&capacity, ReleaseMode::NoCopyBack)
            else {
                return ERR;
            };
            let Ok(mut ch) = env2.get_array_elements_critical(&charge, ReleaseMode::CopyBack)
            else {
                return ERR;
            };
            fill_sequential(supply, &cap, &mut ch)
        };
        let _ = env.set_int_array_region(&out2, 0, &[rest, dem]);
        0
    })
}

/// Variante de `storeRemainingEnergy`; `out` recibe las cargas, devuelve lo que no cupo
/// o `ERR` si falla (no hay ambiguedad: lo que no cabe nunca es negativo).
#[no_mangle]
pub extern "system" fn Java_io_github_thebusybiscuit_slimefun4_core_services_nativeengine_RustNativeEngine_nativeStoreRemaining(
    mut env: JNIEnv,
    _class: JClass,
    remaining: jint,
    capacity: JIntArray,
    out: JIntArray,
) -> jint {
    guard(ERR, || {
        let (Ok(n), Ok(m)) = (env.get_array_length(&capacity), env.get_array_length(&out)) else {
            return ERR;
        };
        if n != m {
            return ERR;
        }
        // SAFETY: igual que arriba.
        unsafe {
            let mut env2 = env.unsafe_clone();
            let Ok(cap) = env.get_array_elements_critical(&capacity, ReleaseMode::NoCopyBack)
            else {
                return ERR;
            };
            let Ok(mut o) = env2.get_array_elements_critical(&out, ReleaseMode::CopyBack) else {
                return ERR;
            };
            store_remaining(remaining, &cap, &mut o)
        }
    })
}

/// Version de ABI para carga por Panama/FFM (sin JNI).
#[no_mangle]
pub extern "C" fn slimefun_abi_version() -> i32 {
    ABI_VERSION
}

/// Reparto por lotes via C ABI (Panama). Mismos semantica y codigos que la variante JNI.
///
/// # Safety
/// `capacity` y `charge` deben apuntar a `len` i32 validos; `out2` a 2 i32 escribibles.
#[no_mangle]
pub unsafe extern "C" fn slimefun_fill_sequential(
    supply: i32,
    capacity: *const i32,
    charge: *mut i32,
    len: usize,
    out2: *mut i32,
) -> i32 {
    if capacity.is_null() || charge.is_null() || out2.is_null() {
        return ERR;
    }
    guard(ERR, || {
        let cap = std::slice::from_raw_parts(capacity, len);
        let ch = std::slice::from_raw_parts_mut(charge, len);
        let (rest, dem) = fill_sequential(supply, cap, ch);
        *out2 = rest;
        *out2.add(1) = dem;
        0
    })
}

/// Variante C ABI (Panama) de `storeRemainingEnergy`: rellena `out` en orden con el sobrante y deja
/// el resto en 0. Devuelve lo que no cupo, o `ERR` si falla. Mismos codigos que la variante JNI.
/// Anadido sin subir la ABI (solo agrega simbolos; quien no los busca no se entera).
///
/// # Safety
/// `capacity` y `out` deben apuntar a `len` i32 validos (`out` escribible).
#[no_mangle]
pub unsafe extern "C" fn slimefun_store_remaining(
    remaining: i32,
    capacity: *const i32,
    out: *mut i32,
    len: usize,
) -> i32 {
    if capacity.is_null() || out.is_null() {
        return ERR;
    }
    guard(ERR, || {
        let cap = std::slice::from_raw_parts(capacity, len);
        let o = std::slice::from_raw_parts_mut(out, len);
        store_remaining(remaining, cap, o)
    })
}

/// Variante C ABI (Panama) del precio de mercado. Misma formula que la JNI; NaN si falla.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn slimefun_market_price(
    base_price: f64,
    demand: i64,
    total_wealth: f64,
    reference_wealth: f64,
    minimum_factor: f64,
    maximum_factor: f64,
    demand_step: f64,
    maximum_demand_factor: f64,
    pulse_factor: f64,
) -> f64 {
    guard(f64::NAN, || {
        calculate_market_price(
            base_price, demand, total_wealth, reference_wealth, minimum_factor,
            maximum_factor, demand_step, maximum_demand_factor, pulse_factor,
        )
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_thebusybiscuit_slimefun4_core_services_nativeengine_RustNativeEngine_nativeSumSaturating(
    mut env: JNIEnv,
    _class: JClass,
    values: JIntArray,
) -> jint {
    jni_sum(&mut env, &values)
}

#[no_mangle]
pub extern "system" fn Java_com_drakescraft_suites_core_nativeengine_NativeEngineBridge_nativeSumSaturating(
    mut env: JNIEnv,
    _class: JClass,
    values: JIntArray,
) -> jint {
    jni_sum(&mut env, &values)
}

#[no_mangle]
pub extern "system" fn Java_com_drakescraft_suites_core_nativeengine_NativeEngineBridge_nativeCheckBossAura(
    _env: JNIEnv,
    _class: JClass,
    bx: jdouble,
    by: jdouble,
    bz: jdouble,
    px: jdouble,
    py: jdouble,
    pz: jdouble,
    radius: jdouble,
) -> jint {
    let dx = bx - px;
    let dy = by - py;
    let dz = bz - pz;
    if (dx * dx + dy * dy + dz * dz) <= (radius * radius) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "system" fn Java_com_drakescraft_suites_core_nativeengine_NativeEngineBridge_nativeEvaluateClock(
    _env: JNIEnv,
    _class: JClass,
    _x: jint,
    _y: jint,
    _z: jint,
    _is_structure: jint,
    is_protected_sf: jint,
) -> jint {
    if is_protected_sf != 0 {
        0 // ALLOW
    } else {
        0
    }
}

#[no_mangle]
pub extern "system" fn Java_io_github_thebusybiscuit_slimefun4_core_services_nativeengine_RustNativeEngine_nativeCalculateMarketPrice(
    _env: JNIEnv,
    _class: JClass,
    base_price: jdouble,
    demand: jlong,
    total_wealth: jdouble,
    reference_wealth: jdouble,
    minimum_factor: jdouble,
    maximum_factor: jdouble,
    demand_step: jdouble,
    maximum_demand_factor: jdouble,
    pulse_factor: jdouble,
) -> jdouble {
    calculate_market_price(
        base_price,
        demand,
        total_wealth,
        reference_wealth,
        minimum_factor,
        maximum_factor,
        demand_step,
        maximum_demand_factor,
        pulse_factor,
    )
}

#[allow(clippy::too_many_arguments)]
fn calculate_market_price(
    base_price: f64,
    demand: i64,
    total_wealth: f64,
    reference_wealth: f64,
    minimum_factor: f64,
    maximum_factor: f64,
    demand_step: f64,
    maximum_demand_factor: f64,
    pulse_factor: f64,
) -> f64 {
    let safe_reference = reference_wealth.max(1.0);
    let wealth_ratio = total_wealth.max(0.0) / safe_reference;
    let wealth_factor = 0.90 + wealth_ratio.ln_1p() / 11.0_f64.ln() * 0.65;
    let demand_factor = maximum_demand_factor.min(1.0 + demand.max(0) as f64 * demand_step);
    let combined =
        (wealth_factor * demand_factor * pulse_factor).clamp(minimum_factor, maximum_factor);
    (base_price * combined * 100.0).round().max(1.0) / 100.0
}

/// Libera la memoria de cadenas C devueltas por Rust.
#[no_mangle]
pub extern "C" fn slimefun_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            let _ = CString::from_raw(s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{calculate_market_price, guard, slimefun_fill_sequential, sum_saturating, ERR};

    #[test]
    fn sums_regular_values() {
        assert_eq!(sum_saturating(&[10, 20, 30]), 60);
    }

    #[test]
    fn saturates_overflow_and_underflow() {
        assert_eq!(sum_saturating(&[i32::MAX, 1]), i32::MAX);
        assert_eq!(sum_saturating(&[i32::MIN, -1]), i32::MIN);
        assert_eq!(sum_saturating(&[i32::MAX, 1, -1]), i32::MAX - 1);
    }

    #[test]
    fn calculates_bounded_market_price() {
        assert_eq!(
            calculate_market_price(100.0, 0, 0.0, 100_000_000.0, 0.85, 1.85, 0.02, 1.45, 1.0),
            90.0
        );
        assert_eq!(
            calculate_market_price(
                100.0,
                1_000,
                1_000_000_000.0,
                100_000_000.0,
                0.85,
                1.85,
                0.02,
                1.45,
                1.0,
            ),
            185.0
        );
    }

    #[test]
    fn un_panic_no_cruza_la_frontera() {
        assert_eq!(guard(ERR, || -> i32 { panic!("prueba") }), ERR);
    }

    #[test]
    fn c_abi_fill_sequential_y_punteros_nulos() {
        let cap = [100, 100];
        let mut ch = [0, 100];
        let mut out = [0, 0];
        let r = unsafe { slimefun_fill_sequential(30, cap.as_ptr(), ch.as_mut_ptr(), 2, out.as_mut_ptr()) };
        assert_eq!((r, ch, out), (0, [30, 100], [0, 100]));
        let r = unsafe { slimefun_fill_sequential(1, std::ptr::null(), ch.as_mut_ptr(), 2, out.as_mut_ptr()) };
        assert_eq!(r, ERR);
    }

    #[test]
    fn c_abi_store_remaining_y_precio() {
        let cap = [100, 100, 100];
        let mut out = [9, 9, 9];
        let left = unsafe { super::slimefun_store_remaining(150, cap.as_ptr(), out.as_mut_ptr(), 3) };
        assert_eq!((left, out), (0, [100, 50, 0]));
        let nulo = unsafe { super::slimefun_store_remaining(1, std::ptr::null(), out.as_mut_ptr(), 3) };
        assert_eq!(nulo, ERR);
        let p = super::slimefun_market_price(100.0, 0, 0.0, 100_000_000.0, 0.85, 1.85, 0.02, 1.45, 1.0);
        assert_eq!(p, 90.0);
    }
}
