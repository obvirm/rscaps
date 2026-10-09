//! Animation recipes as absolute-time evaluators.
//!
//! tscaps recipes are CSS keyframe animations frozen mid-flight by the
//! engine (`animation-play-state: paused` + a delay var = `start − now`),
//! so the visible state at frame time `now` is `keyframe(clamp(now −
//! start))`. Here each recipe is a pure function of the elapsed time —
//! no state, no clock, identical every frame and identical in export.
//!
//! Source facts: `templates/_lib/animation/rise-in/_index.scss` —
//! `from { opacity: var(--entrance-fade); transform: translateY(var(--entrance-rise)) }`
//! `to   { opacity: 1; translateY(0) }`, fill `both`, easing
//! `cubic-bezier(0.22, 1, 0.36, 1)`, duration `0.32s`.

/// Evaluate a cubic-bezier timing function (CSS `cubic-bezier(x1,y1,x2,y2)`
/// with the implicit `P0 = (0,0)`, `P3 = (1,1)`): given progress `x`
/// ∈ [0,1], solve `x(t) = x` by bisection and return `y(t)`.
///
/// Valid easing control points keep `x(t)` strictly increasing
/// (`x1, x2 ∈ [0,1]`), so the solution is unique.
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32) -> impl Fn(f32) -> f32 {
    move |x: f32| {
        if x <= 0.0 { return 0.0; }
        if x >= 1.0 { return 1.0; }
        let x_at = |t: f32| {
            let u = 1.0 - t;
            3.0 * u * u * t * x1 + 3.0 * u * t * t * x2 + t * t * t
        };
        let y_at = |t: f32| {
            let u = 1.0 - t;
            3.0 * u * u * t * y1 + 3.0 * u * t * t * y2 + t * t * t
        };
        let x = x.clamp(0.0, 1.0);
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..40 {
            let mid = (lo + hi) * 0.5;
            if x_at(mid) < x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        y_at((lo + hi) * 0.5)
    }
}

/// `cubic-bezier(0.22, 1, 0.36, 1)` — the default easing of every
/// `_lib/animation` entrance recipe.
pub fn ease_entrance(x: f32) -> f32 {
    cubic_bezier(0.22, 1.0, 0.36, 1.0)(x)
}

/// CSS `ease`.
pub fn ease(x: f32) -> f32 {
    cubic_bezier(0.25, 0.1, 0.25, 1.0)(x)
}

/// CSS `ease-out`.
pub fn ease_out(x: f32) -> f32 {
    cubic_bezier(0.0, 0.0, 0.58, 1.0)(x)
}

/// CSS `ease-in-out`.
pub fn ease_in_out(x: f32) -> f32 {
    cubic_bezier(0.42, 0.0, 0.58, 1.0)(x)
}

/// `cubic-bezier(0.34, 1.35, 0.64, 1)` — `scale-in` default.
pub fn ease_scale_in(x: f32) -> f32 {
    cubic_bezier(0.34, 1.35, 0.64, 1.0)(x)
}

/// `cubic-bezier(0.34, 1.56, 0.64, 1)` — `snap-in` default.
pub fn ease_snap(x: f32) -> f32 {
    cubic_bezier(0.34, 1.56, 0.64, 1.0)(x)
}

/// `cubic-bezier(0.22, 0.4, 0.25, 1)` — `settle-in` default.
pub fn ease_settle(x: f32) -> f32 {
    cubic_bezier(0.22, 0.4, 0.25, 1.0)(x)
}

/// Normalized progress of a multi-stop keyframe: which interval `p` falls
/// in (`stops` ascending, first must be 0.0), returned as local progress
/// in that interval.
fn interval(p: f32, stops: &[f32], index: usize) -> f32 {
    let a = stops[index];
    let b = stops.get(index + 1).copied().unwrap_or(1.0);
    if b <= a {
        return if p >= b { 1.0 } else { 0.0 };
    }
    ((p - a) / (b - a)).clamp(0.0, 1.0)
}

fn which_interval(p: f32, stops: &[f32]) -> usize {
    let mut idx = 0;
    for (i, &s) in stops.iter().enumerate().skip(1) {
        if p >= s {
            idx = i;
        }
    }
    idx
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Shared entry clamp: negative elapsed holds the first keyframe (fill
/// `both`), past-duration holds the last.
fn progress(elapsed_s: f32, duration_s: f32) -> f32 {
    if elapsed_s <= 0.0 {
        0.0
    } else if duration_s <= 0.0 || elapsed_s >= duration_s {
        1.0
    } else {
        elapsed_s / duration_s
    }
}

/// `rise-in`: the element travels `$distance` down into place while fading
/// in from `$from_opacity`, over `$duration_s`, triggered by the caller's
/// delay (elio: the word's narration start).
#[derive(Debug, Clone, Copy)]
pub struct RiseIn {
    pub distance_em: f32,
    pub from_opacity: f32,
    pub duration_s: f32,
    pub ease: fn(f32) -> f32,
}

impl RiseIn {
    /// Recipe defaults from `rise-in/_index.scss` (the `$distance` and
    /// `$from-opacity` are per-template arguments).
    pub fn with(distance_em: f32, from_opacity: f32) -> Self {
        Self {
            distance_em,
            from_opacity,
            duration_s: 0.32,
            ease: ease_entrance,
        }
    }

    /// Fully specified (levi: `0.28s`, `ease-out`).
    pub fn custom(distance_em: f32, from_opacity: f32, duration_s: f32, ease: fn(f32) -> f32) -> Self {
        Self {
            distance_em,
            from_opacity,
            duration_s,
            ease,
        }
    }

    /// `(opacity, translateY_em)` at `elapsed_s` since the trigger.
    /// Negative elapsed (before narration start) holds the `from` keyframe
    /// — CSS fill-mode `both`.
    pub fn eval(&self, elapsed_s: f32) -> (f32, f32) {
        let p = progress(elapsed_s, self.duration_s);
        let p = (self.ease)(p);
        (
            self.from_opacity + (1.0 - self.from_opacity) * p,
            self.distance_em * (1.0 - p),
        )
    }
}

/// `fade-in`: plain opacity over the window (tito 0.25s, yuki 0.18s —
/// CSS `ease`).
#[derive(Debug, Clone, Copy)]
pub struct FadeIn {
    pub from_opacity: f32,
    pub duration_s: f32,
    pub ease: fn(f32) -> f32,
}

impl FadeIn {
    pub fn with(from_opacity: f32, duration_s: f32) -> Self {
        Self {
            from_opacity,
            duration_s,
            ease,
        }
    }

    /// `opacity` at `elapsed_s` since the trigger.
    pub fn eval(&self, elapsed_s: f32) -> f32 {
        let p = progress(elapsed_s, self.duration_s);
        lerp(self.from_opacity, 1.0, (self.ease)(p))
    }
}

/// `scale-in`: arrives at `$from_scale` and settles to 1, optional
/// overshoot `$peak` at `$peak_at` (default 60%). Opacity fades across the
/// whole duration; the transform eases per interval (CSS semantics).
#[derive(Debug, Clone, Copy)]
pub struct ScaleIn {
    pub from_scale: f32,
    pub from_opacity: f32,
    pub peak: Option<f32>,
    pub peak_at: f32,
    pub duration_s: f32,
    pub ease: fn(f32) -> f32,
}

impl ScaleIn {
    pub fn with(from_scale: f32, from_opacity: f32) -> Self {
        Self {
            from_scale,
            from_opacity,
            peak: None,
            peak_at: 0.6,
            duration_s: 0.32,
            ease: ease_scale_in,
        }
    }

    pub fn peak(mut self, peak: f32) -> Self {
        self.peak = Some(peak);
        self
    }

    /// `(opacity, scale)` at `elapsed_s` since the trigger.
    pub fn eval(&self, elapsed_s: f32) -> (f32, f32) {
        let p = progress(elapsed_s, self.duration_s);
        let opacity = lerp(self.from_opacity, 1.0, (self.ease)(p));
        let scale = match self.peak {
            None => lerp(self.from_scale, 1.0, (self.ease)(p)),
            Some(peak) => {
                let stops = [0.0, self.peak_at];
                let i = which_interval(p, &stops);
                if i == 0 {
                    lerp(self.from_scale, peak, (self.ease)(interval(p, &stops, 0)))
                } else {
                    lerp(peak, 1.0, (self.ease)(interval(p, &stops, 1)))
                }
            }
        };
        (opacity, scale)
    }
}

/// `slide-in`: horizontal travel + fade (distance signed: negative enters
/// from the left). Defaults: 0.4s, entrance easing.
#[derive(Debug, Clone, Copy)]
pub struct SlideIn {
    pub distance_em: f32,
    pub from_opacity: f32,
    pub duration_s: f32,
    pub ease: fn(f32) -> f32,
}

impl SlideIn {
    pub fn with(distance_em: f32, from_opacity: f32) -> Self {
        Self {
            distance_em,
            from_opacity,
            duration_s: 0.4,
            ease: ease_entrance,
        }
    }

    /// `(opacity, translateX_em)` at `elapsed_s` since the trigger.
    pub fn eval(&self, elapsed_s: f32) -> (f32, f32) {
        let p = progress(elapsed_s, self.duration_s);
        let p = (self.ease)(p);
        (
            lerp(self.from_opacity, 1.0, p),
            self.distance_em * (1.0 - p),
        )
    }
}

/// `pop-in`: scale overshoot with no fade — 0% `scale(1)`, 40%
/// `scale(1 + amount)`, 100% `scale(1)`, entrance easing, 0.35s.
#[derive(Debug, Clone, Copy)]
pub struct PopIn {
    pub amount: f32,
    pub duration_s: f32,
    pub ease: fn(f32) -> f32,
}

impl PopIn {
    pub fn with(amount: f32) -> Self {
        Self {
            amount,
            duration_s: 0.35,
            ease: ease_entrance,
        }
    }

    /// `scale` at `elapsed_s` since the trigger (1.0 outside the window).
    pub fn eval(&self, elapsed_s: f32) -> f32 {
        let p = progress(elapsed_s, self.duration_s);
        let stops = [0.0, 0.4];
        let i = which_interval(p, &stops);
        if i == 0 {
            lerp(1.0, 1.0 + self.amount, (self.ease)(interval(p, &stops, 0)))
        } else {
            lerp(1.0 + self.amount, 1.0, (self.ease)(interval(p, &stops, 1)))
        }
    }
}

/// `snap-in`: arrives tilted, small and low, overshoots past rest and
/// springs back — stops at 0 / 60% / 100%, `cubic-bezier(0.34, 1.56, 0.64, 1)`,
/// 0.36s. The overshoot is a fixed proportion of `$rise/$rotation/$scale`.
#[derive(Debug, Clone, Copy)]
pub struct SnapIn {
    pub rise_em: f32,
    pub rotation_deg: f32,
    pub scale: f32,
    pub from_opacity: f32,
    pub duration_s: f32,
    pub ease: fn(f32) -> f32,
}

impl SnapIn {
    /// Defaults from `snap-in/_index.scss`.
    pub fn with(rise_em: f32) -> Self {
        Self {
            rise_em,
            rotation_deg: 7.0,
            scale: 0.7,
            from_opacity: 0.0,
            duration_s: 0.36,
            ease: ease_snap,
        }
    }

    /// `(opacity, translateY_em, rotate_deg, scale)` since the trigger.
    pub fn eval(&self, elapsed_s: f32) -> (f32, f32, f32, f32) {
        let p = progress(elapsed_s, self.duration_s);
        let stops = [0.0, 0.6];
        let i = which_interval(p, &stops);
        let e = (self.ease)(interval(p, &stops, i));
        if i == 0 {
            (
                lerp(self.from_opacity, 1.0, e),
                lerp(self.rise_em, self.rise_em * -0.13, e),
                lerp(-self.rotation_deg, self.rotation_deg * 0.43, e),
                lerp(self.scale, 1.0 + (1.0 - self.scale) * 0.33, e),
            )
        } else {
            (
                1.0,
                lerp(self.rise_em * -0.13, 0.0, e),
                lerp(self.rotation_deg * 0.43, 0.0, e),
                lerp(1.0 + (1.0 - self.scale) * 0.33, 1.0, e),
            )
        }
    }
}

/// `wobble-in`: damped rotation — stops 0 / 30 / 55 / 78 / 100% toward
/// `$rest`, `ease-in-out`, 0.6s. `$swing` is the first swing's amplitude
/// away from the rest angle.
#[derive(Debug, Clone, Copy)]
pub struct WobbleIn {
    pub swing_deg: f32,
    pub rest_deg: f32,
    pub duration_s: f32,
    pub ease: fn(f32) -> f32,
}

impl WobbleIn {
    pub fn with(swing_deg: f32, rest_deg: f32) -> Self {
        Self {
            swing_deg,
            rest_deg,
            duration_s: 0.6,
            ease: ease_in_out,
        }
    }

    /// `rotate_deg` at `elapsed_s` since the trigger.
    pub fn eval(&self, elapsed_s: f32) -> f32 {
        let p = progress(elapsed_s, self.duration_s);
        let stops = [0.0, 0.3, 0.55, 0.78];
        let values = [
            self.rest_deg + self.swing_deg,
            self.rest_deg + self.swing_deg * -0.45,
            self.rest_deg + self.swing_deg * 0.22,
            self.rest_deg + self.swing_deg * -0.11,
            self.rest_deg,
        ];
        let i = which_interval(p, &stops);
        lerp(values[i], values[i + 1], (self.ease)(interval(p, &stops, i)))
    }
}

/// `settle-in`: travels while fading, reaching full opacity at the
/// midpoint so the text is readable before it stops moving. Stops at
/// 0 / 50 / 100%, `cubic-bezier(0.22, 0.4, 0.25, 1)`, 0.35s.
#[derive(Debug, Clone, Copy)]
pub struct SettleIn {
    pub distance_em: f32,
    pub from_opacity: f32,
    pub duration_s: f32,
    pub ease: fn(f32) -> f32,
}

impl SettleIn {
    /// Defaults from `settle-in/_index.scss` (`$distance` per template).
    pub fn with(distance_em: f32) -> Self {
        Self {
            distance_em,
            from_opacity: 0.8,
            duration_s: 0.35,
            ease: ease_settle,
        }
    }

    /// `(opacity, translateY_em)` at `elapsed_s` since the trigger.
    pub fn eval(&self, elapsed_s: f32) -> (f32, f32) {
        let p = progress(elapsed_s, self.duration_s);
        // transform keys at 0%/100% → one eased interval; opacity keys at
        // 0%/50%/100% → eased to the midpoint, then flat.
        let ty = lerp(self.distance_em, 0.0, (self.ease)(p));
        let opacity = if p < 0.5 {
            lerp(self.from_opacity, 1.0, (self.ease)(interval(p, &[0.0, 0.5], 0)))
        } else {
            1.0
        };
        (opacity, ty)
    }
}

/// `bob`: looping idle float while the element is on screen —
/// 0/50/100% → `0 / −distance / 0`, `ease-in-out`, `$duration` infinite.
/// Unlike entrances this loops: a silenced entrance does not freeze it.
#[derive(Debug, Clone, Copy)]
pub struct Bob {
    pub distance_em: f32,
    pub duration_s: f32,
    pub ease: fn(f32) -> f32,
    /// Segment start + per-word stagger (tito: `+ word_index × 0.10s`).
    pub delay_s: f32,
}

impl Bob {
    /// Defaults from `bob/_index.scss`.
    pub fn with(distance_em: f32, delay_s: f32) -> Self {
        Self {
            distance_em,
            duration_s: 1.6,
            ease: ease_in_out,
            delay_s,
        }
    }

    /// `translateY_em` at `now_s` (looping; before the delay holds 0).
    pub fn eval(&self, now_s: f32) -> f32 {
        let t = now_s - self.delay_s;
        if t < 0.0 {
            return 0.0;
        }
        let phase = (t % self.duration_s) / self.duration_s;
        if phase < 0.5 {
            lerp(0.0, -self.distance_em, (self.ease)(phase * 2.0))
        } else {
            lerp(-self.distance_em, 0.0, (self.ease)((phase - 0.5) * 2.0))
        }
    }
}

// ── steps-eased recipes (glitch-in, jitter) ────────────────────────────────

/// CSS `steps(2)` (= `steps(2, end)`): a hard-cut easing that holds the
/// interval's start value for its first half, then the midpoint, landing
/// on 1 only at the end. `glitch-in` and `jitter` pair it with their
/// opposed offsets so the motion reads as cuts, not slides.
pub fn steps2(x: f32) -> f32 {
    if x >= 1.0 {
        1.0
    } else {
        (x * 2.0).floor() * 0.5
    }
}

/// `glitch-in`: the element appears displaced, kicks to the opposite
/// offset, then lands — stops 0 / 50 / 100%, `steps(2)`, 0.22s, fading
/// in from `$from-opacity` alongside.
#[derive(Debug, Clone, Copy)]
pub struct GlitchIn {
    pub x_em: f32,
    pub y_em: f32,
    pub from_opacity: f32,
    pub duration_s: f32,
}

impl GlitchIn {
    /// Defaults from `glitch-in/_index.scss` (`$x`/`$y` per template).
    pub fn with(x_em: f32, y_em: f32) -> Self {
        Self {
            x_em,
            y_em,
            from_opacity: 0.0,
            duration_s: 0.22,
        }
    }

    /// `(opacity, translateX_em, translateY_em)` at `elapsed_s`.
    pub fn eval(&self, elapsed_s: f32) -> (f32, f32, f32) {
        let p = progress(elapsed_s, self.duration_s);
        let stops = [0.0, 0.5];
        let i = which_interval(p, &stops);
        let e = steps2(interval(p, &stops, i));
        if i == 0 {
            (
                lerp(self.from_opacity, 1.0, e),
                lerp(self.x_em, -self.x_em, e),
                lerp(self.y_em, -self.y_em, e),
            )
        } else {
            (1.0, lerp(-self.x_em, 0.0, e), lerp(-self.y_em, 0.0, e))
        }
    }
}

/// `jitter`: small irregular shake (emphasis while narrated, not an
/// entrance) — stops 0 / 20 / 40 / 60 / 100%, `steps(2, end)`, 0.3s.
#[derive(Debug, Clone, Copy)]
pub struct Jitter {
    pub x_em: f32,
    pub y_em: f32,
    pub duration_s: f32,
}

impl Jitter {
    /// Defaults from `jitter/_index.scss`.
    pub fn with(x_em: f32, y_em: f32) -> Self {
        Self {
            x_em,
            y_em,
            duration_s: 0.3,
        }
    }

    /// `(translateX_em, translateY_em)` at `elapsed_s`.
    pub fn eval(&self, elapsed_s: f32) -> (f32, f32) {
        let p = progress(elapsed_s, self.duration_s);
        let stops = [0.0, 0.2, 0.4, 0.6];
        let values = [
            (0.0, 0.0),
            (-self.x_em, self.y_em),
            (self.x_em, -self.y_em),
            (-self.x_em * 0.6, 0.0),
            (0.0, 0.0),
        ];
        let i = which_interval(p, &stops);
        let e = steps2(interval(p, &stops, i));
        (
            lerp(values[i].0, values[i + 1].0, e),
            lerp(values[i].1, values[i + 1].1, e),
        )
    }
}

// ── looping / colour recipes (shimmer, highlight-pulse, flash-in) ──────────

/// `shimmer`: a sweep that loops for the element's whole life — linear
/// phase `[0,1)` of the cycle (the caller maps it onto a gradient
/// offset). 6s per cycle by default.
#[derive(Debug, Clone, Copy)]
pub struct Shimmer {
    pub duration_s: f32,
}

impl Shimmer {
    /// Default from `shimmer/_index.scss`.
    pub fn with() -> Self {
        Self { duration_s: 6.0 }
    }

    /// Phase of the loop at absolute `now_s` (`0` before the delay is
    /// the caller's concern — loops have no resting state).
    pub fn eval(&self, now_s: f32) -> f32 {
        let t = now_s % self.duration_s;
        (if t < 0.0 { t + self.duration_s } else { t }) / self.duration_s
    }
}

/// `highlight-pulse`: the word swells to the highlight colour across its
/// narration window and returns — stops 0 / 20 / 80 / 100% (rest →
/// hold → rest), `ease-in-out`, over the word's own window.
#[derive(Debug, Clone, Copy)]
pub struct HighlightPulse {
    pub hold_from: f32,
    pub hold_to: f32,
}

impl HighlightPulse {
    /// Defaults from `highlight-pulse/_index.scss`.
    pub fn with() -> Self {
        Self {
            hold_from: 0.2,
            hold_to: 0.8,
        }
    }

    /// Mix factor at `elapsed_s` within the word's `duration_s`:
    /// `0.0` = resting colour, `1.0` = highlight colour.
    pub fn eval(&self, elapsed_s: f32, duration_s: f32) -> f32 {
        let p = progress(elapsed_s, duration_s);
        if p < self.hold_from {
            ease_in_out(p / self.hold_from)
        } else if p <= self.hold_to {
            1.0
        } else {
            1.0 - ease_in_out((p - self.hold_to) / (1.0 - self.hold_to))
        }
    }
}

/// What `flash-in` paints at a frame: the element hides (not fades —
/// the plate must stay lit over it) until the plate cuts away, then
/// hops in. The plate holds solid 8%–58% so some frame always lands
/// inside the hold at 24–30fps.
#[derive(Debug, Clone, Copy)]
pub struct FlashState {
    pub element_visible: bool,
    pub element_opacity: f32,
    pub plate_opacity: f32,
}

/// `flash-in`: an opaque plate strikes the element's box and the element
/// arrives out of it — 0.1s, plate `linear`, element `ease`.
#[derive(Debug, Clone, Copy)]
pub struct FlashIn {
    pub duration_s: f32,
}

impl FlashIn {
    /// Default from `flash-in/_index.scss`.
    pub fn with() -> Self {
        Self { duration_s: 0.1 }
    }

    /// The state at `elapsed_s` since the trigger (fill `both`).
    pub fn eval(&self, elapsed_s: f32) -> FlashState {
        let p = progress(elapsed_s, self.duration_s);
        // Element: opacity keys 0%→1, 58%→1, 62%→0, 100%→1, eased per
        // interval; visibility hops on at 58%.
        let element_opacity = if p <= 0.58 {
            1.0
        } else if p < 0.62 {
            lerp(1.0, 0.0, ease((p - 0.58) / 0.04))
        } else {
            lerp(0.0, 1.0, ease((p - 0.62) / 0.38))
        };
        // Plate: 0%→0, 8%→1, 58%→1, 62%→0, 100%→0, linear.
        let plate_opacity = if p < 0.08 {
            p / 0.08
        } else if p <= 0.58 {
            1.0
        } else if p < 0.62 {
            1.0 - (p - 0.58) / 0.04
        } else {
            0.0
        };
        FlashState {
            element_visible: p >= 0.58,
            element_opacity,
            plate_opacity,
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_solver_is_consistent_with_its_parametric_curve() {
        let ease = cubic_bezier(0.22, 1.0, 0.36, 1.0);
        // For a parametric t, x(t) must map back to y(t).
        for t in [0.1f32, 0.25, 0.5, 0.75, 0.9] {
            let u = 1.0 - t;
            let x = 3.0 * u * u * t * 0.22 + 3.0 * u * t * t * 0.36 + t * t * t;
            let y = 3.0 * u * u * t * 1.0 + 3.0 * u * t * t * 1.0 + t * t * t;
            assert!(
                (ease(x) - y).abs() < 1e-3,
                "x({t})={x}: got {}, want {y}",
                ease(x)
            );
        }
        assert!((ease(0.0)).abs() < 1e-6);
        assert!((ease(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn entrance_ease_is_monotonic_and_fast_out() {
        // cubic-bezier(0.22, 1, 0.36, 1) front-loads: half-time progress
        // is well past half.
        assert!(ease_entrance(0.5) > 0.7);
        let mut prev = 0.0;
        for x in [0.1f32, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0] {
            let y = ease_entrance(x);
            assert!(y >= prev);
            prev = y;
        }
    }

    #[test]
    fn rise_in_holds_the_from_keyframe_before_the_trigger() {
        let rise = RiseIn::with(0.29, 0.0);
        let (opacity, dy) = rise.eval(-1.0);
        assert_eq!(opacity, 0.0);
        assert_eq!(dy, 0.29);
    }

    #[test]
    fn rise_in_lands_exactly_after_the_duration() {
        let rise = RiseIn::with(0.29, 0.0);
        let (opacity, dy) = rise.eval(0.32);
        assert_eq!(opacity, 1.0);
        assert!((dy - 0.0).abs() < 1e-6);
        // Stays landed (fill forwards).
        let (opacity, dy) = rise.eval(5.0);
        assert_eq!(opacity, 1.0);
        assert!((dy - 0.0).abs() < 1e-6);
    }

    #[test]
    fn rise_in_midpoint_is_eased_not_linear() {
        let rise = RiseIn::with(0.29, 0.0);
        let (opacity, dy) = rise.eval(0.16);
        assert!(opacity > 0.5, "front-loaded easing, got {opacity}");
        assert!((dy - 0.29 * (1.0 - opacity)).abs() < 1e-5);
    }

    #[test]
    fn fade_in_fades_over_its_duration_and_holds() {
        // yuki: 0.18s, from 0.
        let fade = FadeIn::with(0.0, 0.18);
        assert_eq!(fade.eval(-0.5), 0.0);
        assert!((fade.eval(0.18) - 1.0).abs() < 1e-6);
        assert!((fade.eval(9.0) - 1.0).abs() < 1e-6);
        // ease front-loads.
        assert!(fade.eval(0.09) > 0.5);
        // tito: 0.25s.
        let fade = FadeIn::with(0.0, 0.25);
        assert_eq!(fade.eval(0.3), 1.0);
    }

    #[test]
    fn scale_in_settles_to_one_and_peaks_when_asked() {
        let plain = ScaleIn::with(0.6, 0.0);
        let (o, s) = plain.eval(-1.0);
        assert_eq!(o, 0.0);
        assert!((s - 0.6).abs() < 1e-6);
        let (o, s) = plain.eval(0.32);
        assert!((o - 1.0).abs() < 1e-6);
        assert!((s - 1.0).abs() < 1e-6);

        let peaked = ScaleIn::with(0.5, 0.0).peak(1.3);
        let (_, s) = peaked.eval(0.32 * 0.6);
        assert!((s - 1.3).abs() < 1e-5, "peak at peak_at, got {s}");
        let (_, s) = peaked.eval(0.32 * 0.8);
        assert!((s - 1.0).abs() < 0.31, "coming back down through {s}");
    }

    #[test]
    fn slide_in_enters_from_the_signed_side() {
        let left = SlideIn::with(-0.5, 0.0);
        let (o, tx) = left.eval(-1.0);
        assert_eq!(o, 0.0);
        assert!((tx + 0.5).abs() < 1e-6);
        let (_, tx) = left.eval(0.4);
        assert!(tx.abs() < 1e-6);
    }

    #[test]
    fn pop_in_overshoots_at_forty_percent() {
        let pop = PopIn::with(0.1);
        assert!((pop.eval(-1.0) - 1.0).abs() < 1e-6);
        assert!((pop.eval(0.35 * 0.4) - 1.1).abs() < 1e-5);
        assert!((pop.eval(0.35) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn snap_in_travels_springs_and_lands() {
        let snap = SnapIn::with(0.4);
        let (o, ty, rot, sc) = snap.eval(-1.0);
        assert_eq!(o, 0.0);
        assert!((ty - 0.4).abs() < 1e-6);
        assert!((rot + 7.0).abs() < 1e-6);
        assert!((sc - 0.7).abs() < 1e-6);
        // 60% stop exactly.
        let (o, ty, rot, sc) = snap.eval(0.36 * 0.6);
        assert!((o - 1.0).abs() < 1e-6);
        assert!((ty + 0.4 * 0.13).abs() < 1e-5);
        assert!((rot - 7.0 * 0.43).abs() < 1e-5);
        assert!((sc - (1.0 + 0.3 * 0.33)).abs() < 1e-5);
        // Landed.
        let (o, ty, rot, sc) = snap.eval(5.0);
        assert_eq!(o, 1.0);
        assert!(ty.abs() < 1e-6 && rot.abs() < 1e-6 && (sc - 1.0).abs() < 1e-6);
    }

    #[test]
    fn wobble_decays_to_the_rest_angle() {
        let w = WobbleIn::with(10.0, 2.0);
        assert!((w.eval(-1.0) - 12.0).abs() < 1e-6);
        let mid1 = w.eval(0.6 * 0.3);
        let mid2 = w.eval(0.6 * 0.55);
        let mid3 = w.eval(0.6 * 0.78);
        assert!((mid1 + 2.5).abs() < 1e-4, "{mid1}");
        assert!((mid2 - 4.2).abs() < 1e-4, "{mid2}");
        assert!((mid3 - 0.9).abs() < 1e-4, "{mid3}");
        assert!((w.eval(5.0) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn settle_in_is_readable_at_the_midpoint() {
        let s = SettleIn::with(0.3);
        assert!((s.from_opacity - 0.8).abs() < 1e-6);
        let (o, ty) = s.eval(-1.0);
        assert!((o - 0.8).abs() < 1e-6);
        assert!((ty - 0.3).abs() < 1e-6);
        let (o, ty) = s.eval(0.35 * 0.5);
        assert!((o - 1.0).abs() < 1e-6, "full opacity at midpoint");
        assert!(ty > 0.0 && ty < 0.15, "travel eased, got {ty}");
        let (o, ty) = s.eval(5.0);
        assert_eq!(o, 1.0);
        assert!(ty.abs() < 1e-6);
    }

    #[test]
    fn bob_loops_periodically_and_staggers_by_delay() {
        let b = Bob::with(0.19, 0.5);
        assert_eq!(b.eval(-1.0), 0.0);
        assert_eq!(b.eval(0.4), 0.0);
        // Periodic.
        for t in [0.6f32, 1.0, 1.3, 2.7] {
            assert!((b.eval(t) - b.eval(t + 1.6)).abs() < 1e-6, "t={t}");
        }
        // Peak lift at mid-cycle of the first run (delay .5 + .8).
        assert!((b.eval(0.5 + 0.8) + 0.19).abs() < 1e-4);
        // Zero at cycle boundaries.
        assert!(b.eval(0.5).abs() < 1e-6);
        assert!(b.eval(0.5 + 1.6).abs() < 1e-6);
    }

    #[test]
    fn levi_line_rise_uses_its_own_duration_and_easing() {
        let rise = RiseIn::custom(0.0, 0.0, 0.28, ease_out);
        let (o, dy) = rise.eval(-0.1);
        assert_eq!(o, 0.0);
        assert_eq!(dy, 0.0);
        let (o, _) = rise.eval(0.28);
        assert!((o - 1.0).abs() < 1e-6);
        // ease-out: more than half done at half time.
        let (o, _) = rise.eval(0.14);
        assert!(o > 0.6, "front-loaded ease-out, got {o}");
    }

    #[test]
    fn steps2_quantizes_each_interval_to_two_cuts() {
        assert_eq!(steps2(0.0), 0.0);
        assert_eq!(steps2(0.24), 0.0);
        assert_eq!(steps2(0.5), 0.5);
        assert_eq!(steps2(0.75), 0.5);
        assert_eq!(steps2(1.0), 1.0);
    }

    #[test]
    fn glitch_in_kicks_to_the_opposite_offset_and_lands() {
        let g = GlitchIn::with(0.03, -0.02);
        let (o, tx, ty) = g.eval(-1.0);
        assert_eq!(o, 0.0);
        assert!((tx - 0.03).abs() < 1e-6 && (ty + 0.02).abs() < 1e-6);
        // First half of interval 0 (steps(2) holds the start value) and
        // the midpoint value in its second half.
        let (_, tx, _) = g.eval(0.22 * 0.24);
        assert!((tx - 0.03).abs() < 1e-6, "held by steps, got {tx}");
        let (_, tx, ty) = g.eval(0.22 * 0.375);
        assert!((tx - 0.0).abs() < 1e-6 && (ty - 0.0).abs() < 1e-6, "{tx},{ty}");
        // From the 50% stop the opposite offset applies (pure for the
        // interval's first half).
        let (o, tx, ty) = g.eval(0.22 * 0.6);
        assert!((o - 1.0).abs() < 1e-6);
        assert!((tx + 0.03).abs() < 1e-6 && (ty - 0.02).abs() < 1e-6, "{tx},{ty}");
        // Landed.
        let (o, tx, ty) = g.eval(1.0);
        assert_eq!(o, 1.0);
        assert!(tx.abs() < 1e-6 && ty.abs() < 1e-6);
    }

    #[test]
    fn jitter_shakes_in_asymmetric_cuts_and_returns_to_rest() {
        let j = Jitter::with(0.02, 0.012);
        assert_eq!(j.eval(0.0), (0.0, 0.0));
        // Pure keyframe states land in each interval's first half.
        let (tx, ty) = j.eval(0.3 * 0.25);
        assert!((tx + 0.02).abs() < 1e-6 && (ty - 0.012).abs() < 1e-6, "{tx},{ty}");
        let (tx, ty) = j.eval(0.3 * 0.45);
        assert!((tx - 0.02).abs() < 1e-6 && (ty + 0.012).abs() < 1e-6, "{tx},{ty}");
        // Second half of an interval shows the steps midpoint.
        let (tx, ty) = j.eval(0.3 * 0.35);
        assert!((tx - 0.0).abs() < 1e-6 && (ty - 0.0).abs() < 1e-6, "{tx},{ty}");
        assert_eq!(j.eval(1.0), (0.0, 0.0));
    }

    #[test]
    fn shimmer_loops_a_linear_six_second_phase() {
        let s = Shimmer::with();
        assert_eq!(s.eval(0.0), 0.0);
        assert!((s.eval(3.0) - 0.5).abs() < 1e-6);
        assert!((s.eval(6.0)).abs() < 1e-6);
        assert!((s.eval(7.5) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn highlight_pulse_holds_the_plateau_and_returns_to_rest() {
        let h = HighlightPulse::with();
        assert!(h.eval(0.0, 1.0) < 1e-6);
        assert_eq!(h.eval(0.3, 1.0), 1.0);
        assert_eq!(h.eval(0.7, 1.0), 1.0);
        assert!(h.eval(1.0, 1.0) < 1e-6);
        // Swelling into the plateau is eased, not linear.
        assert!(h.eval(0.1, 1.0) > 0.1);
        assert!(h.eval(0.1, 1.0) < 0.9);
    }

    #[test]
    fn flash_in_holds_the_plate_then_snaps_the_element_in() {
        let f = FlashIn::with();
        let s = f.eval(0.0);
        assert!(!s.element_visible && (s.plate_opacity - 0.0).abs() < 1e-6);
        let s = f.eval(0.1 * 0.3);
        assert!(!s.element_visible && (s.plate_opacity - 1.0).abs() < 1e-6);
        let s = f.eval(0.1 * 0.58);
        assert!(s.element_visible && (s.plate_opacity - 1.0).abs() < 1e-6);
        let s = f.eval(0.1 * 0.62);
        assert!(s.element_visible && (s.plate_opacity).abs() < 1e-6 && s.element_opacity < 1e-3);
        let s = f.eval(0.1);
        assert!(s.element_visible && (s.element_opacity - 1.0).abs() < 1e-6);
    }
}
