//! Query functions over metric series: per-series transforms (rate,
//! increase) and cross-series aggregations (sum/avg/min/max).

use otelview_model::{MetricSeries, SeriesPoint};
use serde_json::json;

/// Per-series transform applied point-by-point.
/// - `rate`: per-second derivative between consecutive points (counter
///   resets clamp to the new value over the interval)
/// - `increase`: delta between consecutive points (resets clamp to 0)
/// - anything else: raw values untouched
pub fn apply_function(series: &mut [MetricSeries], func: &str) {
    if func != "rate" && func != "increase" {
        return;
    }
    for s in series.iter_mut() {
        s.points.sort_by_key(|p| p.time_unix_nano);
        let mut out = Vec::with_capacity(s.points.len().saturating_sub(1));
        for w in s.points.windows(2) {
            let dt = (w[1].time_unix_nano.saturating_sub(w[0].time_unix_nano)) as f64 / 1e9;
            if dt <= 0.0 {
                continue;
            }
            let dv = w[1].value - w[0].value;
            let dv = if dv < 0.0 {
                // Counter reset: assume it restarted from zero.
                if func == "rate" { w[1].value } else { 0.0 }
            } else {
                dv
            };
            out.push(SeriesPoint {
                time_unix_nano: w[1].time_unix_nano,
                value: if func == "rate" { dv / dt } else { dv },
            });
        }
        s.points = out;
    }
}

/// Combine all series into one by aligning points into `buckets` time slots
/// (mean within a slot per series) and folding across series with `agg`
/// (sum | avg | min | max). Anything else returns the input unchanged.
pub fn aggregate(series: Vec<MetricSeries>, agg: &str, buckets: usize) -> Vec<MetricSeries> {
    if !matches!(agg, "sum" | "avg" | "min" | "max") || series.is_empty() {
        return series;
    }
    let t_min = series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.time_unix_nano))
        .min()
        .unwrap_or(0);
    let t_max = series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.time_unix_nano))
        .max()
        .unwrap_or(0)
        .max(t_min + 1);
    let buckets = buckets.clamp(2, 2_000) as u64;
    let width = ((t_max - t_min) / buckets).max(1);

    // slot → per-series mean values
    let mut slots: Vec<Vec<f64>> = vec![Vec::new(); buckets as usize];
    for s in &series {
        let mut sums = vec![(0.0f64, 0u64); buckets as usize];
        for p in &s.points {
            let idx = (p.time_unix_nano.saturating_sub(t_min) / width).min(buckets - 1) as usize;
            sums[idx].0 += p.value;
            sums[idx].1 += 1;
        }
        for (idx, (sum, count)) in sums.into_iter().enumerate() {
            if count > 0 {
                slots[idx].push(sum / count as f64);
            }
        }
    }

    let points: Vec<SeriesPoint> = slots
        .into_iter()
        .enumerate()
        .filter(|(_, values)| !values.is_empty())
        .map(|(i, values)| {
            let value = match agg {
                "sum" => values.iter().sum(),
                "avg" => values.iter().sum::<f64>() / values.len() as f64,
                "min" => values.iter().cloned().fold(f64::INFINITY, f64::min),
                _ => values.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            };
            SeriesPoint { time_unix_nano: t_min + i as u64 * width + width / 2, value }
        })
        .collect();

    vec![MetricSeries {
        service_name: agg.to_string(),
        attributes: json!({ "aggregation": agg, "series": series.len() }),
        points,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(points: &[(u64, f64)]) -> MetricSeries {
        MetricSeries {
            service_name: "svc".into(),
            attributes: json!({}),
            points: points
                .iter()
                .map(|(t, v)| SeriesPoint { time_unix_nano: *t, value: *v })
                .collect(),
        }
    }

    #[test]
    fn rate_is_per_second_and_handles_resets() {
        let mut s = vec![series(&[
            (0, 100.0),
            (1_000_000_000, 160.0),  // +60 over 1s → 60/s
            (2_000_000_000, 10.0),   // reset → 10/s
            (4_000_000_000, 30.0),   // +20 over 2s → 10/s
        ])];
        apply_function(&mut s, "rate");
        let v: Vec<f64> = s[0].points.iter().map(|p| p.value).collect();
        assert_eq!(v, vec![60.0, 10.0, 10.0]);
    }

    #[test]
    fn increase_clamps_resets_to_zero() {
        let mut s = vec![series(&[(0, 100.0), (1_000_000_000, 160.0), (2_000_000_000, 10.0)])];
        apply_function(&mut s, "increase");
        let v: Vec<f64> = s[0].points.iter().map(|p| p.value).collect();
        assert_eq!(v, vec![60.0, 0.0]);
    }

    #[test]
    fn sum_aggregates_across_series() {
        let out = aggregate(
            vec![
                series(&[(0, 1.0), (1_000, 2.0)]),
                series(&[(0, 10.0), (1_000, 20.0)]),
            ],
            "sum",
            2,
        );
        assert_eq!(out.len(), 1);
        let v: Vec<f64> = out[0].points.iter().map(|p| p.value).collect();
        assert_eq!(v, vec![11.0, 22.0]);
        assert_eq!(out[0].attributes["aggregation"], "sum");
    }

    #[test]
    fn unknown_function_and_agg_are_noops() {
        let mut s = vec![series(&[(0, 1.0), (1, 2.0)])];
        apply_function(&mut s, "raw");
        assert_eq!(s[0].points.len(), 2);
        let out = aggregate(s, "none", 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].points.len(), 2);
    }
}
