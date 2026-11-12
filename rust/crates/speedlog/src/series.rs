//! Time series of one field and their reduction for a visible window.
use std::sync::Arc;

use crate::types::{SeriesView, Stats};

/// Time (seconds) and value of every record of a field, in file order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Series {
    /// Shared by every field of the same message type (and instance).
    pub t: Arc<Vec<f64>>,
    pub v: Vec<f64>,
}

fn lower_bound(x: &[f64], v: f64) -> usize {
    x.partition_point(|a| *a < v)
}

/// First/last raw index spanning the visible window, one sample of margin on each side so the
/// line reaches the plot edges.
fn visible_bounds(x: &[f64], range: Option<(f64, f64)>) -> (usize, usize) {
    let n = x.len();
    match range {
        None => (0, n),
        Some((a, b)) => (
            lower_bound(x, a).saturating_sub(1),
            n.min(lower_bound(x, b) + 1),
        ),
    }
}

/// The visible window of a series, reduced to the min and max of `buckets` buckets so big logs
/// stay fast while short spikes are kept; plus min/max/mean of the raw samples in the window.
pub fn view(series: &Series, range: Option<(f64, f64)>, buckets: usize) -> SeriesView {
    let (lo, hi) = visible_bounds(&series.t, range);
    let hi = hi.max(lo);
    let (t, v) = (&series.t[lo..hi], &series.v[lo..hi]);
    let buckets = buckets.max(1);

    let (out_t, out_v) = if t.len() <= buckets * 3 {
        (t.to_vec(), v.to_vec())
    } else {
        let mut out_t = Vec::with_capacity(buckets * 2);
        let mut out_v = Vec::with_capacity(buckets * 2);
        let step = t.len() as f64 / buckets as f64;
        for b in 0..buckets {
            let start = (b as f64 * step) as usize;
            let end = (((b + 1) as f64 * step) as usize).min(t.len());
            let mut min_i: Option<usize> = None;
            let mut max_i: Option<usize> = None;
            for i in start..end {
                let x = v[i];
                if !x.is_finite() {
                    continue;
                }
                if min_i.is_none_or(|m| x < v[m]) {
                    min_i = Some(i);
                }
                if max_i.is_none_or(|m| x > v[m]) {
                    max_i = Some(i);
                }
            }
            let (Some(a), Some(b)) = (min_i, max_i) else {
                continue;
            };
            let (first, second) = (a.min(b), a.max(b));
            out_t.push(t[first]);
            out_v.push(v[first]);
            if second != first {
                out_t.push(t[second]);
                out_v.push(v[second]);
            }
        }
        (out_t, out_v)
    };

    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut sum = 0.0;
    let mut count = 0usize;
    for x in v.iter().copied().filter(|x| x.is_finite()) {
        min = min.min(x);
        max = max.max(x);
        sum += x;
        count += 1;
    }
    SeriesView {
        t: out_t,
        v: out_v,
        stats: (count > 0).then(|| Stats {
            min,
            max,
            mean: sum / count as f64,
        }),
        t_first: series.t.first().copied(),
        t_last: series.t.last().copied(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(n: usize) -> Series {
        Series {
            t: Arc::new((0..n).map(|i| i as f64).collect()),
            v: (0..n).map(|i| i as f64).collect(),
        }
    }

    #[test]
    fn small_window_is_returned_raw() {
        let s = ramp(10);
        let out = view(&s, None, 100);
        assert_eq!(out.t, *s.t);
        let st = out.stats.unwrap();
        assert_eq!((st.min, st.max, st.mean), (0.0, 9.0, 4.5));
        assert_eq!(out.t_first, Some(0.0));
        assert_eq!(out.t_last, Some(9.0));
    }

    #[test]
    fn window_keeps_one_sample_of_margin() {
        let s = ramp(10);
        let out = view(&s, Some((3.5, 5.5)), 100);
        assert_eq!(out.t, [3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn reduction_keeps_spikes() {
        let mut s = ramp(100_000);
        s.v.iter_mut().for_each(|v| *v = 0.0);
        s.v[54_321] = 1000.0;
        s.v[77_777] = -1000.0;
        let out = view(&s, None, 100);
        assert!(out.t.len() <= 200);
        assert!(out.v.contains(&1000.0));
        assert!(out.v.contains(&-1000.0));
        // Time stays sorted, so the plot draws in order
        assert!(out.t.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn nan_samples_are_skipped() {
        let s = Series {
            t: Arc::new(vec![0.0, 1.0, 2.0]),
            v: vec![f64::NAN, 2.0, 4.0],
        };
        let st = view(&s, None, 10).stats.unwrap();
        assert_eq!(st.mean, 3.0);
        let empty = Series {
            t: Arc::new(vec![0.0]),
            v: vec![f64::NAN],
        };
        assert!(view(&empty, None, 10).stats.is_none());
    }

    #[test]
    fn window_outside_the_series_is_empty_or_edge() {
        let s = ramp(10);
        let out = view(&s, Some((100.0, 200.0)), 10);
        assert_eq!(out.t, [9.0]);
        let out = view(&s, Some((-20.0, -10.0)), 10);
        assert_eq!(out.t, [0.0]);
    }
}
