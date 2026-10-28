//! Reparto de energía de una red de Slimefun.
//!
//! El orden importa y es el de Slimefun: **primero se cubre el consumo con lo que se genera en
//! este tick**, y solo lo que sobra carga capacitores. Si no llega, la diferencia sale de los
//! capacitores. Hacerlo al revés (todo al capacitor y luego repartir) haría que una red con
//! capacitores llenos desperdiciara generación mientras las máquinas se quedan a medias.
//!
//! Toda la aritmética es saturante a propósito. Una red grande puede sumar producciones que
//! desbordan `u64`, y un desbordamiento silencioso aquí se traduce en máquinas que se apagan sin
//! motivo aparente; saturar es feo pero visible y no rompe la invariante.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EnergyNodeType {
    Generator { production_per_tick: u64 },
    Capacitor { capacity: u64, stored: u64 },
    Consumer { demand_per_tick: u64 },
    Connector,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnergyNode {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub node_type: EnergyNodeType,
}

/// Qué pasó en un tick. Se devuelve entero en vez de solo la generación porque lo que interesa
/// diagnosticar es justo lo otro: cuánta demanda quedó sin cubrir y cuánta energía se tiró.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EnergyReport {
    /// Producido por los generadores en este tick.
    pub generated: u64,
    /// Demanda total de los consumidores.
    pub demanded: u64,
    /// Demanda efectivamente cubierta, con generación o con reservas.
    pub supplied: u64,
    /// Lo que entró en capacitores.
    pub charged: u64,
    /// Lo que salió de capacitores para tapar el déficit.
    pub discharged: u64,
    /// Sobrante que no cupo en ningún capacitor y se pierde.
    pub wasted: u64,
    /// Demanda que no se pudo cubrir ni con generación ni con reservas.
    pub unmet: u64,
}

impl EnergyReport {
    /// Si la red va justa. Útil para avisar en vez de dejar máquinas parándose en silencio.
    pub fn is_starved(&self) -> bool {
        self.unmet > 0
    }
}

#[derive(Default)]
pub struct EnergyNetGraphSolver {
    nodes: DashMap<(i32, i32, i32), EnergyNode>,
}

impl EnergyNetGraphSolver {
    pub fn new() -> Self {
        Self {
            nodes: DashMap::new(),
        }
    }

    pub fn register_node(&self, x: i32, y: i32, z: i32, node_type: EnergyNodeType) {
        let node = EnergyNode { x, y, z, node_type };
        self.nodes.insert((x, y, z), node);
    }

    pub fn remove_node(&self, x: i32, y: i32, z: i32) -> Option<EnergyNode> {
        self.nodes.remove(&(x, y, z)).map(|(_, n)| n)
    }

    /// Energía acumulada ahora mismo en todos los capacitores.
    pub fn stored_energy(&self) -> u64 {
        self.nodes
            .iter()
            .filter_map(|r| match r.value().node_type {
                EnergyNodeType::Capacitor { stored, .. } => Some(stored),
                _ => None,
            })
            .fold(0u64, |a, b| a.saturating_add(b))
    }

    /// Resuelve un tick completo: genera, reparte, carga y descarga.
    ///
    /// Es secuencial. Se hizo así a sabiendas: cargar y descargar capacitores muta estado
    /// compartido, y repartirlo entre hilos haría que el resultado dependiera del orden en que
    /// los toca cada hilo. Una red de este tamaño se resuelve en microsegundos; el paralelismo
    /// solo aportaría no-determinismo.
    pub fn solve_tick(&self) -> EnergyReport {
        let mut report = EnergyReport::default();

        for r in self.nodes.iter() {
            match r.value().node_type {
                EnergyNodeType::Generator {
                    production_per_tick,
                } => report.generated = report.generated.saturating_add(production_per_tick),
                EnergyNodeType::Consumer { demand_per_tick } => {
                    report.demanded = report.demanded.saturating_add(demand_per_tick)
                }
                _ => {}
            }
        }

        if report.generated >= report.demanded {
            report.supplied = report.demanded;
            let surplus = report.generated - report.demanded;
            report.charged = self.charge(surplus);
            report.wasted = surplus - report.charged;
        } else {
            let deficit = report.demanded - report.generated;
            report.discharged = self.discharge(deficit);
            report.supplied = report.generated.saturating_add(report.discharged);
            report.unmet = deficit - report.discharged;
        }

        report
    }

    /// Reparte `amount` entre los capacitores con hueco. Devuelve lo que realmente entró.
    fn charge(&self, amount: u64) -> u64 {
        let mut left = amount;

        for mut r in self.nodes.iter_mut() {
            if left == 0 {
                break;
            }
            if let EnergyNodeType::Capacitor { capacity, stored } = &mut r.value_mut().node_type {
                // Un capacitor puede quedar por encima de su capacidad si alguien la baja en
                // caliente; saturar evita que la resta se desborde y le regale energía.
                let room = capacity.saturating_sub(*stored);
                let take = room.min(left);
                *stored += take;
                left -= take;
            }
        }

        amount - left
    }

    /// Saca hasta `amount` de los capacitores. Devuelve lo que realmente salió.
    fn discharge(&self, amount: u64) -> u64 {
        let mut left = amount;

        for mut r in self.nodes.iter_mut() {
            if left == 0 {
                break;
            }
            if let EnergyNodeType::Capacitor { stored, .. } = &mut r.value_mut().node_type {
                let take = (*stored).min(left);
                *stored -= take;
                left -= take;
            }
        }

        amount - left
    }

    pub fn count_nodes(&self) -> usize {
        self.nodes.len()
    }
}

/// Reparto por lotes sobre arrays planos (SoA), equivalente exacto del bucle de consumidores de
/// `EnergyNet.tick` en Java: recorre en orden, rellena cada consumidor con hueco y acumula la
/// demanda con suma saturante en `i32` (igual que `NumberUtils.flowSafeAddition`).
///
/// Escribe la carga resultante en `charge` y devuelve `(energia_restante, demanda)`.
/// Sin asignaciones: trabaja sobre los buffers del llamante.
pub fn fill_sequential(supply: i32, capacity: &[i32], charge: &mut [i32]) -> (i32, i32) {
    let mut remaining = supply;
    let mut demand = 0_i32;
    for (cap, cur) in capacity.iter().zip(charge.iter_mut()) {
        if *cur < *cap {
            let space = *cap - *cur;
            demand = demand.saturating_add(space);
            if remaining > 0 {
                if remaining > space {
                    *cur = *cap;
                    remaining -= space;
                } else {
                    *cur += remaining;
                    remaining = 0;
                }
            }
        }
    }
    (remaining, demand)
}

/// Equivalente de `EnergyNet.storeRemainingEnergy` para un grupo: el sobrante llena en orden y
/// el resto queda en 0. Devuelve lo que no cupo.
pub fn store_remaining(remaining: i32, capacity: &[i32], out: &mut [i32]) -> i32 {
    let mut left = remaining;
    for (cap, o) in capacity.iter().zip(out.iter_mut()) {
        if left > 0 {
            if left > *cap {
                *o = *cap;
                left -= *cap;
            } else {
                *o = left;
                left = 0;
            }
        } else {
            *o = 0;
        }
    }
    left
}

/// Suma `i32` con saturacion, igual que `EnergyNet` en Java.
pub fn sum_saturating(values: &[i32]) -> i32 {
    values.iter().copied().fold(0_i32, i32::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solver(nodes: &[EnergyNodeType]) -> EnergyNetGraphSolver {
        let s = EnergyNetGraphSolver::new();
        for (i, t) in nodes.iter().enumerate() {
            s.register_node(i as i32, 0, 0, *t);
        }
        s
    }

    #[test]
    fn el_sobrante_carga_capacitores() {
        let s = solver(&[
            EnergyNodeType::Generator {
                production_per_tick: 100,
            },
            EnergyNodeType::Consumer { demand_per_tick: 30 },
            EnergyNodeType::Capacitor {
                capacity: 1000,
                stored: 0,
            },
        ]);

        let r = s.solve_tick();
        assert_eq!(r.generated, 100);
        assert_eq!(r.supplied, 30);
        assert_eq!(r.charged, 70);
        assert_eq!(r.wasted, 0);
        assert_eq!(s.stored_energy(), 70);
    }

    #[test]
    fn el_deficit_sale_de_los_capacitores() {
        let s = solver(&[
            EnergyNodeType::Generator {
                production_per_tick: 10,
            },
            EnergyNodeType::Consumer { demand_per_tick: 50 },
            EnergyNodeType::Capacitor {
                capacity: 1000,
                stored: 100,
            },
        ]);

        let r = s.solve_tick();
        assert_eq!(r.discharged, 40);
        assert_eq!(r.supplied, 50);
        assert_eq!(r.unmet, 0);
        assert_eq!(s.stored_energy(), 60);
    }

    #[test]
    fn sin_reservas_suficientes_queda_demanda_sin_cubrir() {
        let s = solver(&[
            EnergyNodeType::Generator {
                production_per_tick: 10,
            },
            EnergyNodeType::Consumer {
                demand_per_tick: 100,
            },
            EnergyNodeType::Capacitor {
                capacity: 1000,
                stored: 15,
            },
        ]);

        let r = s.solve_tick();
        assert_eq!(r.discharged, 15, "solo habia 15 guardados");
        assert_eq!(r.supplied, 25, "10 generados mas los 15 de reserva");
        assert_eq!(r.unmet, 75, "del deficit de 90 se taparon 15");
        assert_eq!(r.supplied + r.unmet, r.demanded, "todo lo pedido se explica");
        assert!(r.is_starved());
        assert_eq!(s.stored_energy(), 0);
    }

    #[test]
    fn lo_que_no_cabe_en_capacitores_se_pierde() {
        let s = solver(&[
            EnergyNodeType::Generator {
                production_per_tick: 500,
            },
            EnergyNodeType::Capacitor {
                capacity: 100,
                stored: 90,
            },
        ]);

        let r = s.solve_tick();
        assert_eq!(r.charged, 10);
        assert_eq!(r.wasted, 490);
        assert_eq!(s.stored_energy(), 100);
    }

    #[test]
    fn el_reparto_llena_un_capacitor_antes_de_pasar_al_siguiente() {
        let s = solver(&[
            EnergyNodeType::Generator {
                production_per_tick: 150,
            },
            EnergyNodeType::Capacitor {
                capacity: 100,
                stored: 0,
            },
            EnergyNodeType::Capacitor {
                capacity: 100,
                stored: 0,
            },
        ]);

        let r = s.solve_tick();
        assert_eq!(r.charged, 150);
        assert_eq!(r.wasted, 0);
        assert_eq!(s.stored_energy(), 150);
    }

    #[test]
    fn una_red_de_solo_conectores_no_hace_nada() {
        let s = solver(&[EnergyNodeType::Connector, EnergyNodeType::Connector]);
        assert_eq!(s.solve_tick(), EnergyReport::default());
    }

    #[test]
    fn las_sumas_saturan_en_vez_de_desbordar() {
        let s = solver(&[
            EnergyNodeType::Generator {
                production_per_tick: u64::MAX,
            },
            EnergyNodeType::Generator {
                production_per_tick: u64::MAX,
            },
        ]);

        assert_eq!(s.solve_tick().generated, u64::MAX);
    }

    #[test]
    fn un_capacitor_por_encima_de_su_capacidad_no_regala_energia() {
        // Pasa si alguien baja la capacidad en caliente con el capacitor lleno.
        let s = solver(&[
            EnergyNodeType::Generator {
                production_per_tick: 50,
            },
            EnergyNodeType::Capacitor {
                capacity: 10,
                stored: 80,
            },
        ]);

        let r = s.solve_tick();
        assert_eq!(r.charged, 0, "no cabe nada, la resta no debe desbordar");
        assert_eq!(r.wasted, 50);
        assert_eq!(s.stored_energy(), 80);
    }

    #[test]
    fn quitar_un_nodo_lo_saca_del_reparto() {
        let s = solver(&[
            EnergyNodeType::Generator {
                production_per_tick: 100,
            },
            EnergyNodeType::Consumer { demand_per_tick: 40 },
        ]);
        s.remove_node(1, 0, 0);

        let r = s.solve_tick();
        assert_eq!(r.demanded, 0);
        assert_eq!(r.wasted, 100, "sin capacitores el sobrante se pierde");
    }

    #[test]
    fn fill_sequential_rellena_en_orden_y_suma_demanda() {
        let cap = [100, 100, 100];
        let mut ch = [90, 0, 100];
        let (rest, dem) = fill_sequential(50, &cap, &mut ch);
        assert_eq!(ch, [100, 40, 100]);
        assert_eq!(rest, 0);
        assert_eq!(dem, 10 + 100);
    }

    #[test]
    fn fill_sequential_satura_la_demanda() {
        let cap = [i32::MAX, i32::MAX];
        let mut ch = [0, 0];
        let (rest, dem) = fill_sequential(0, &cap, &mut ch);
        assert_eq!((rest, dem), (0, i32::MAX));
    }

    #[test]
    fn fill_sequential_ignora_cargas_sobre_capacidad() {
        let mut ch = [80];
        assert_eq!(fill_sequential(50, &[10], &mut ch), (50, 0));
        assert_eq!(ch, [80]);
    }

    #[test]
    fn store_remaining_llena_y_pone_a_cero() {
        let mut out = [9, 9, 9];
        assert_eq!(store_remaining(150, &[100, 100, 100], &mut out), 0);
        assert_eq!(out, [100, 50, 0]);
        assert_eq!(store_remaining(500, &[100, 100], &mut out[..2]), 300);
    }
}
