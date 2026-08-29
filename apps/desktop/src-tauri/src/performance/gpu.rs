//! GPU telemetry, for the machine that is now running the model itself.
//!
//! When the brain is somewhere else, the interesting question is how many
//! tokens a second came back. When the brain is in this box, that number is an
//! *effect*, and this module reports the causes: how close the card is to its
//! power limit, how hot it is, how much VRAM is left, and — the one that
//! actually explains a sudden collapse — whether the clocks are being held down
//! and by what.
//!
//! ## Why `nvidia-smi` and not a library
//!
//! NVML would avoid a process spawn, and would also add a native dependency
//! that has to be found, versioned and shipped. `nvidia-smi` is installed with
//! every NVIDIA driver, answers all of this in a single CSV row, and returns in
//! tens of milliseconds. It is sampled at most once a second and cached, so the
//! spawn happens on a timer rather than once per reader.
//!
//! ## Two traps, both hit while building this
//!
//! * **The console window.** A bare `Command::new` on Windows flashes a console
//!   every time it runs. At a one-second cadence that is not a cosmetic issue,
//!   it is unusable. `CREATE_NO_WINDOW` is required, exactly as the shell
//!   lookup in `session::commands` already does.
//! * **`[N/A]` is not zero.** Several fields are unavailable on a consumer card
//!   under Windows' display driver model — per-process VRAM among them,
//!   verified on this machine. Parsing those to `0` would draw a chart of
//!   confident zeroes. They parse to `None` and render as an em dash.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

/// The fields asked for, in order. Kept as one list so the query string and the
/// parser cannot drift apart.
const FIELDS: &[&str] = &[
    "name",
    "driver_version",
    "power.draw",
    "power.limit",
    "power.max_limit",
    "temperature.gpu",
    "utilization.gpu",
    "utilization.memory",
    "memory.used",
    "memory.total",
    "clocks.current.sm",
    "clocks.max.sm",
    "clocks.current.memory",
    "fan.speed",
    "pstate",
    "clocks_event_reasons.sw_power_cap",
    "clocks_event_reasons.hw_slowdown",
    "clocks_event_reasons.hw_thermal_slowdown",
    "clocks_event_reasons.sw_thermal_slowdown",
];

/// How long a sample is served before the card is asked again.
const FRESH_FOR: Duration = Duration::from_millis(900);

/// How much history the sparklines get. At roughly one sample a second this is
/// the last minute, which is the span over which a thermal or power limit
/// actually bites.
const HISTORY: usize = 60;

/// Why the card is not running as fast as it could.
///
/// This is the field that turns "it got slower" into an answer. On a 375 W
/// limit, sustained generation on a 3090 Ti sits against the power cap long
/// before it gets anywhere near a thermal one, and those two call for opposite
/// responses from the person reading it.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThrottleReasons {
    pub power_cap: bool,
    pub hardware_slowdown: bool,
    pub hardware_thermal: bool,
    pub software_thermal: bool,
}

impl ThrottleReasons {
    pub fn any(&self) -> bool {
        self.power_cap || self.hardware_slowdown || self.hardware_thermal || self.software_thermal
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuMetrics {
    pub name: Option<String>,
    pub driver_version: Option<String>,
    /// Watts being drawn right now.
    pub power_draw_watts: Option<f64>,
    /// The limit in force — the user's 375 W, not the card's 450 W ceiling.
    pub power_limit_watts: Option<f64>,
    /// The highest limit this card will accept, so a person can see how much
    /// headroom the current setting leaves on the table.
    pub power_max_watts: Option<f64>,
    pub temperature_c: Option<f64>,
    pub utilization_percent: Option<f64>,
    /// Memory *bandwidth* utilisation, not occupancy. During generation this
    /// is the one that saturates first: a decoder is memory-bound.
    pub memory_utilization_percent: Option<f64>,
    pub memory_used_mib: Option<f64>,
    pub memory_total_mib: Option<f64>,
    pub clock_sm_mhz: Option<f64>,
    pub clock_sm_max_mhz: Option<f64>,
    pub clock_memory_mhz: Option<f64>,
    pub fan_percent: Option<f64>,
    /// `P0` under load, `P8` at idle.
    pub performance_state: Option<String>,
    pub throttle: ThrottleReasons,
    /// Recent history, oldest first, for the surface's sparklines.
    pub power_history: Vec<f64>,
    pub utilization_history: Vec<f64>,
    pub temperature_history: Vec<f64>,
}

impl GpuMetrics {
    /// Share of the power limit currently being used, 0.0–1.0.
    pub fn power_fraction(&self) -> Option<f64> {
        let (draw, limit) = (self.power_draw_watts?, self.power_limit_watts?);
        (limit > 0.0).then(|| (draw / limit).clamp(0.0, 1.0))
    }

    /// VRAM still free, in MiB. What decides whether a second model, or a
    /// longer context, will fit.
    pub fn memory_free_mib(&self) -> Option<f64> {
        Some((self.memory_total_mib? - self.memory_used_mib?).max(0.0))
    }
}

#[derive(Default)]
struct Sampler {
    taken_at: Option<Instant>,
    latest: Option<GpuMetrics>,
    power: Vec<f64>,
    utilization: Vec<f64>,
    temperature: Vec<f64>,
}

static SAMPLER: Mutex<Option<Sampler>> = Mutex::new(None);

/// The current GPU state, or `None` when there is no NVIDIA GPU to ask.
///
/// `None` is a first-class answer: this product runs on machines without a
/// discrete card, and a GPU panel showing zeroes would be worse than one that
/// is honestly absent (§81).
#[tauri::command]
pub fn gpu_metrics() -> Option<GpuMetrics> {
    let mut guard = SAMPLER.lock().ok()?;
    let sampler = guard.get_or_insert_with(Sampler::default);

    let fresh = sampler
        .taken_at
        .map(|taken| taken.elapsed() < FRESH_FOR)
        .unwrap_or(false);
    if fresh {
        return sampler.latest.clone();
    }

    sampler.taken_at = Some(Instant::now());
    let sampled = query().and_then(|output| parse(&output));

    if let Some(mut metrics) = sampled {
        push(&mut sampler.power, metrics.power_draw_watts);
        push(&mut sampler.utilization, metrics.utilization_percent);
        push(&mut sampler.temperature, metrics.temperature_c);
        metrics.power_history = sampler.power.clone();
        metrics.utilization_history = sampler.utilization.clone();
        metrics.temperature_history = sampler.temperature.clone();
        sampler.latest = Some(metrics);
    } else {
        // A single failed sample is not proof the card vanished — a driver can
        // be busy. The last good reading is kept, and only a reading that was
        // never obtained reports nothing.
        sampler.latest = sampler.latest.take();
    }
    sampler.latest.clone()
}

fn push(series: &mut Vec<f64>, value: Option<f64>) {
    let Some(value) = value else { return };
    series.push(value);
    if series.len() > HISTORY {
        let excess = series.len() - HISTORY;
        series.drain(..excess);
    }
}

#[cfg(windows)]
fn query() -> Option<String> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("nvidia-smi")
        .arg(format!("--query-gpu={}", FIELDS.join(",")))
        .arg("--format=csv,noheader,nounits")
        // CREATE_NO_WINDOW — see the module docs. Without it this flashes a
        // console once a second for as long as the HUD is open.
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(not(windows))]
fn query() -> Option<String> {
    let output = std::process::Command::new("nvidia-smi")
        .arg(format!("--query-gpu={}", FIELDS.join(",")))
        .arg("--format=csv,noheader,nounits")
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

/// Read one CSV row into metrics.
///
/// The first row only: a machine with two cards reports two, and silently
/// summing or averaging them would describe a GPU that does not exist.
pub fn parse(output: &str) -> Option<GpuMetrics> {
    let line = output.lines().find(|line| !line.trim().is_empty())?;
    let cells: Vec<&str> = line.split(',').map(str::trim).collect();
    if cells.len() < FIELDS.len() {
        return None;
    }

    let text = |index: usize| -> Option<String> {
        let value = cells[index];
        // `[N/A]`, `[Not Supported]`, `[Unknown Error]` — every unavailable
        // reading nvidia-smi has is bracketed.
        (!value.is_empty() && !value.starts_with('[') && !value.eq_ignore_ascii_case("N/A"))
            .then(|| value.to_string())
    };
    let number = |index: usize| -> Option<f64> { text(index)?.parse().ok() };
    let active = |index: usize| -> bool {
        text(index)
            .map(|value| value.eq_ignore_ascii_case("Active"))
            .unwrap_or(false)
    };

    Some(GpuMetrics {
        name: text(0),
        driver_version: text(1),
        power_draw_watts: number(2),
        power_limit_watts: number(3),
        power_max_watts: number(4),
        temperature_c: number(5),
        utilization_percent: number(6),
        memory_utilization_percent: number(7),
        memory_used_mib: number(8),
        memory_total_mib: number(9),
        clock_sm_mhz: number(10),
        clock_sm_max_mhz: number(11),
        clock_memory_mhz: number(12),
        fan_percent: number(13),
        performance_state: text(14),
        throttle: ThrottleReasons {
            power_cap: active(15),
            hardware_slowdown: active(16),
            hardware_thermal: active(17),
            software_thermal: active(18),
        },
        power_history: Vec::new(),
        utilization_history: Vec::new(),
        temperature_history: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real row from the card in this machine, idle.
    const IDLE: &str = "NVIDIA GeForce RTX 3090 Ti, 616.56, 8.45, 375.00, 450.00, 48, 3, 2, 21686, 24564, 210, 2100, 405, 30, P8, Not Active, Not Active, Not Active, Not Active";

    #[test]
    fn a_real_row_is_read_field_for_field() {
        let metrics = parse(IDLE).expect("the row has every field");
        assert_eq!(metrics.name.as_deref(), Some("NVIDIA GeForce RTX 3090 Ti"));
        assert_eq!(metrics.power_draw_watts, Some(8.45));
        assert_eq!(metrics.power_limit_watts, Some(375.0));
        assert_eq!(metrics.power_max_watts, Some(450.0));
        assert_eq!(metrics.temperature_c, Some(48.0));
        assert_eq!(metrics.memory_used_mib, Some(21686.0));
        assert_eq!(metrics.performance_state.as_deref(), Some("P8"));
        assert!(!metrics.throttle.any());
    }

    /// The user set a 375 W limit on a card that accepts 450 W. Reporting the
    /// draw against the card's ceiling instead of the limit in force would
    /// show a card loafing while it is in fact pinned.
    #[test]
    fn power_is_measured_against_the_limit_in_force_not_the_cards_ceiling() {
        let metrics = parse(IDLE).unwrap();
        let fraction = metrics.power_fraction().unwrap();
        assert!((fraction - 8.45 / 375.0).abs() < 1e-9);
        assert_ne!(metrics.power_limit_watts, metrics.power_max_watts);
    }

    #[test]
    fn free_vram_is_what_is_left_of_the_card() {
        let metrics = parse(IDLE).unwrap();
        assert_eq!(metrics.memory_free_mib(), Some(24564.0 - 21686.0));
    }

    /// The same card, **measured while it was generating tokens**.
    ///
    /// Not a synthetic row: this was read off this machine mid-response from a
    /// 27B Q4 model. It is the whole reason the throttle line exists, and the
    /// values are worth reading — the card is pinned at 374.56 W of its 375 W
    /// limit with `sw_power_cap` Active, while every thermal reason stays
    /// inactive at 80 °C. Those two call for opposite responses from the person
    /// looking at them, so the surface must never merge them.
    const LOADED: &str = "NVIDIA GeForce RTX 3090 Ti, 616.56, 374.56, 375.00, 450.00, 80, 94, 70, 21633, 24564, 1740, 2100, 10251, 72, P2, Active, Not Active, Not Active, Not Active";

    #[test]
    fn a_power_capped_card_says_so_and_does_not_blame_the_temperature() {
        let metrics = parse(LOADED).expect("a real loaded row parses");

        assert!(metrics.throttle.power_cap);
        assert!(metrics.throttle.any());
        assert!(
            !metrics.throttle.hardware_thermal && !metrics.throttle.software_thermal,
            "80 C on this card is hot and is not what is holding it back; \
             reporting a thermal limit here would send somebody to buy fans \
             when the fix is the power limit"
        );

        // Pinned, not loafing: the surface's power meter has to reach the top.
        let fraction = metrics.power_fraction().expect("both values were read");
        assert!(fraction > 0.99, "374.56 W of 375 W is against the limit");

        // And there is real headroom left in the card itself, which is the
        // actionable part of "held back by the power limit".
        assert!(metrics.power_max_watts.unwrap() > metrics.power_limit_watts.unwrap());
    }

    /// Idle and loaded must not look alike.
    #[test]
    fn an_idle_card_is_not_reported_as_constrained() {
        let idle = parse(IDLE).unwrap();
        let loaded = parse(LOADED).unwrap();

        assert!(!idle.throttle.any());
        assert!(loaded.throttle.any());
        assert!(idle.power_fraction().unwrap() < loaded.power_fraction().unwrap());
        assert_eq!(idle.performance_state.as_deref(), Some("P8"));
        assert_eq!(loaded.performance_state.as_deref(), Some("P2"));
    }

    /// `[N/A]` must never become a confident zero.
    #[test]
    fn an_unavailable_field_is_absent_rather_than_zero() {
        let row = IDLE.replace(", 30, P8", ", [N/A], P8");
        let metrics = parse(&row).unwrap();
        assert_eq!(metrics.fan_percent, None);
        assert_eq!(
            metrics.temperature_c,
            Some(48.0),
            "one missing field must not discard the rest of the row"
        );
    }

    #[test]
    fn a_machine_with_no_nvidia_gpu_reports_nothing() {
        assert!(parse("").is_none());
        assert!(parse("some driver error").is_none());
    }

    /// Two cards report two rows; describing them as one would invent a GPU.
    #[test]
    fn only_the_first_card_is_described() {
        let two = format!("{IDLE}\n{}", IDLE.replace("21686", "1024"));
        assert_eq!(parse(&two).unwrap().memory_used_mib, Some(21686.0));
    }

    #[test]
    fn history_is_bounded() {
        let mut series = Vec::new();
        for value in 0..(HISTORY * 2) {
            push(&mut series, Some(value as f64));
        }
        assert_eq!(series.len(), HISTORY);
        assert_eq!(
            series.last(),
            Some(&((HISTORY * 2 - 1) as f64)),
            "the newest sample survives; the oldest is dropped"
        );
    }
}
