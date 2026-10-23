# Recoil-aware aiming

The aimbot and standalone recoil helper now share one controller and one mouse
output. No view-angle writes are used. Target selection and the orange FOV marker
use the direction formed by the local view angles plus the measured recoil.

## Live data

The recoil reader resolves fields from Valthrun's runtime schema:

- New layout: `C_CSPlayerPawn.m_pAimPunchServices` points to
  `CCSPlayer_AimPunchServices.m_predictableBaseAngle`.
- Legacy layout: `C_CSPlayerPawn.m_aimPunchAngle`.

Missing fields, unreadable data, null service pointers, and non-finite samples
produce an error instead of guessed recoil or fixed offsets. QAngle reads use
three floats. The debug window names the source being used.

Recoil is scaled by `weapon_recoil_scale`. Conversion to relative mouse counts
uses `sensitivity`, `m_pitch`, `m_yaw`, and the pawn's FOV sensitivity adjustment.
Strength 1.0 requests full measured-recoil compensation; 0.0 disables that axis.

This is measured recoil state, but not a measurement of an individual bullet's
impact. In particular, the new service stores a base angle at an engine timestamp.
This implementation does not reproduce its between-tick decay/interpolation or
the separate unpredictable/damage punch component. It also does not reconstruct
random spread, command timing, or server hit registration. Those limitations need
in-game measurement before claiming precise shot alignment. The former
`velocity * tick_fraction * 0.1` extrapolation was removed rather than treated as
an established engine formula.

## Ownership and smoothing

With a target, aim correction uses target angle minus view angle minus recoil.
When continuing a target, the change in recoil is compensated without applying
aim smoothing to that change. Target acquisition remains smoothed. The correction
formula accounts for recoil already included in the angular error, avoiding a
second full recoil subtraction.

Without a target, Simple Recoil Helper uses only the change since the previous
sample and honors `aim_assist_recoil_min_bullets`. Sampling continues while a
target is active, so losing the target does not replay an entire spray. State is
reset for menu pauses, disabling both features, invalid player state, and pawn or
weapon changes. Fractional mouse counts survive idle samples.

## Earlier lock controls

Commit `1c2625e` implemented these controls; `2291ec4` removed them before this
change:

- `aimbot_lock_strength` was a screen-pixel tolerance, not an attraction gain.
  It stopped correction inside that radius. A fixed pixel radius can exceed a
  distant target's apparent size, so it is not restored.
- `aimbot_strict_boundary` moved halfway toward that tolerance circle's edge,
  not toward the bone or a verified hitbox center. It is not a hitbox constraint.
- `aimbot_distance_scaling` increased smoothing up to 2.5 times and scaled the
  tolerance separately. Optional distance-based aim smoothing is compatible with
  the new controller, but should never scale the recoil correction. It has not
  been restored as part of this change.

Actual hitbox containment would require hitbox geometry and bone transforms;
the current model exposes bone positions, not individual hitbox volumes.

## References inspected

- [TKazer combined aiming and RCS](https://github.com/TKazer/CS2_External/blob/master/CS2_External/AimBot.hpp):
  angle-based target correction and legacy punch-cache reads. This implementation
  writes view angles and is an older reference, not a drop-in implementation.
- [a2x current schema dump](https://github.com/a2x/cs2-dumper/blob/main/output/client_dll.hpp):
  aim-punch service fields, weapon recoil parameters, and recoil/spread seeds.
- [GFusion aimbot source](https://github.com/Cr0mb/CS2-GFusion-Python/blob/main/Features/aimbot.py):
  reads the newer service base-angle chain with a legacy fallback. Only the recoil
  field relationship was used as a reference.
- [Artanis-RCS](https://github.com/ArtanisInc/Artanis-RCS): stored weapon patterns;
  these are an alternative input model, not evidence of a more accurate live
  recoil measurement. No patterns or external executable code were imported.

## Verification

Regression cases cover shot direction away from the crosshair, angle wrapping,
mouse signs and sensitivity, recoil changes under different smoothing factors,
repeated samples, resets and weapon changes, modern/legacy recoil layouts, null
pointers, and invalid samples.

Nightly formatting and `git diff --check` passed. The attempted
`cargo +nightly check -p controller --tests --locked` stopped at the missing
Visual C++ linker (`link.exe`), before checking the controller. Tests and gameplay
are not yet verified.

After installing the C++ toolchain and Windows SDK:

```text
cargo +nightly check -p controller --tests --locked
cargo +nightly test -p controller enhancements::aim --locked
cargo +nightly test -p cs2 state::recoil --locked
```

In-game validation should compare the orange shot marker with recoil-following
crosshair and impact diagnostics, then test stationary fire, moving targets,
crouching, scope transitions, target loss, new sprays, and weapon switches.
Check standalone, target-only, and both-enabled modes separately. Full measured
compensation uses RCS strengths of 1.0 on both axes.
