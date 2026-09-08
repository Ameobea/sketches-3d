//! Crease measurements at existing sampling guides, taken from the final evaluated profile.
//! These are local tangent discontinuities, not the dihedral angles of the swept 3D surface.

use crate::Vec2;

/// Turns at or above this angle receive the old full critical-pair attraction. Below it the
/// attraction fades smoothly. This is independent of the cutoff for mandatory sampling.
const FULL_STRENGTH_RADIANS: f32 = 15. * std::f32::consts::PI / 180.;

#[derive(Clone, Copy, Debug)]
pub(super) struct ProfileCrease {
  pub t: f32,
  /// Signed turn in radians. None means the local geometry could not be measured reliably.
  pub turn: Option<f32>,
}

impl ProfileCrease {
  pub fn strength(self) -> f32 {
    let Some(turn) = self.turn else {
      // Uncertain is not the same as smooth: retain the old behavior for degenerate probes.
      return 1.;
    };
    let x = (turn.abs() / FULL_STRENGTH_RADIANS).clamp(0., 1.);
    x * x * (3. - 2. * x)
  }

  pub fn retain(self, threshold: f32) -> bool {
    // Parameter boundaries are still required even when they are not geometric creases.
    self.t == 0. || self.t == 1. || self.turn.is_none_or(|v| v.abs() >= threshold)
  }
}

/// `guides` must be sorted, distinct, and include 0 and 1. Probes stay inside the neighboring
/// guide intervals, independently of the requested mesh resolution. Extrapolating one-sided
/// secants removes their leading smooth-curvature contribution (plain secants would mistake
/// smooth curves for corners). All measurements use the final sampler, including lerps/scales.
pub(super) fn measure_profile_creases<E>(
  guides: &[f32],
  closed: bool,
  sample: impl Fn(f32) -> Result<Vec2, E>,
) -> Result<Vec<ProfileCrease>, E> {
  let mut out = Vec::with_capacity(guides.len());
  for (i, &t) in guides.iter().enumerate() {
    if i == guides.len() - 1 && closed {
      let first: ProfileCrease = out[0];
      out.push(ProfileCrease { t, ..first });
      continue;
    }
    if !closed && (i == 0 || i == guides.len() - 1) {
      out.push(ProfileCrease { t, turn: Some(0.) });
      continue;
    }
    let left = if i == 0 {
      guides[guides.len() - 2] - 1.
    } else {
      guides[i - 1]
    };
    let right = guides[i + 1];
    let h = ((t - left).min(right - t) * 0.125).min(0.01);
    if h < 4. * f32::EPSILON {
      out.push(ProfileCrease { t, turn: None });
      continue;
    }
    let eval = |s: f32| {
      sample(if s < 0. {
        s + 1.
      } else if s > 1. {
        s - 1.
      } else {
        s
      })
    };
    let p = eval(t)?;
    let lm = eval(t - h)?;
    let ln = eval(t - h * 0.5)?;
    let rn = eval(t + h * 0.5)?;
    let rm = eval(t + h)?;
    // Both vectors point in increasing-t direction; common h factors cancel in atan2.
    let incoming = (p - ln) * 4. - (p - lm);
    let outgoing = (rn - p) * 4. - (rm - p);
    let magnitude = [p, lm, ln, rn, rm]
      .iter()
      .fold(0f32, |m, p| m.max(p.x.abs()).max(p.y.abs()));
    let noise = magnitude * f32::EPSILON * 16.;
    let turn = if incoming.norm() <= noise || outgoing.norm() <= noise {
      None
    } else {
      let angle =
        (incoming.x * outgoing.y - incoming.y * outgoing.x).atan2(incoming.dot(&outgoing));
      angle.is_finite().then_some(angle)
    };
    out.push(ProfileCrease { t, turn });
  }
  Ok(out)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn measured_turn_fades_cancels_and_respects_transforms() {
    let measure = |mix: f32, y_scale: f32| {
      measure_profile_creases(&[0., 0.5, 1.], false, |t| {
        let x = 2. * t - 1.;
        // Interpolate opposite V profiles; their turns cancel at mix=0.5.
        Ok::<_, ()>(Vec2::new(x, (1. - 2. * mix) * x.abs() * y_scale))
      })
      .unwrap()[1]
    };
    assert!((measure(0., 1.).turn.unwrap() - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
    assert!(measure(0.5, 1.).turn.unwrap().abs() < 1e-5);
    assert!(measure(1., 1.).turn.unwrap() < 0.);
    assert!(measure(0.499, 1.).strength() < 0.001);
    assert!(!measure(0.499, 1.).retain(1f32.to_radians()));
    assert!(measure(0.49, 10.).strength() > measure(0.49, 1.).strength());
  }

  #[test]
  fn smooth_guides_closed_seams_and_degenerate_probes() {
    let guides = [0., 0.25, 0.5, 0.75, 1.];
    let circle = measure_profile_creases(&guides, true, |t| {
      let a = t * std::f32::consts::TAU;
      Ok::<_, ()>(Vec2::new(a.cos(), a.sin()))
    })
    .unwrap();
    assert!(circle.iter().all(|c| c.turn.unwrap().abs() < 0.001));
    let square = measure_profile_creases(&guides, true, |t| {
      let corners = [
        Vec2::new(0., 0.),
        Vec2::new(1., 0.),
        Vec2::new(1., 1.),
        Vec2::new(0., 1.),
        Vec2::new(0., 0.),
      ];
      let s = (t * 4.).min(3.999999);
      let i = s as usize;
      Ok::<_, ()>(corners[i].lerp(&corners[i + 1], s - i as f32))
    })
    .unwrap();
    assert!(square
      .iter()
      .all(|c| (c.turn.unwrap() - std::f32::consts::FRAC_PI_2).abs() < 0.001));
    let collapsed = measure_profile_creases(&guides, true, |_| Ok::<_, ()>(Vec2::zeros())).unwrap();
    assert!(collapsed
      .iter()
      .all(|c| c.turn.is_none() && c.retain(0.1) && c.strength() == 1.));
  }
}
