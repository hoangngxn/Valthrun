use anyhow::Context;
use cs2::{
    read_aim_punch,
    schema::ConVar,
    StateCS2Memory,
    StateEntityList,
    StateLocalPlayerController,
};
use cs2_schema_generated::cs2::client::{
    C_BaseEntity,
    C_BaseModelEntity,
    C_BasePlayerPawn,
};
use nalgebra::Vector3;
use raw_struct::Reference;

/// Angular values are degrees, ordered [pitch, yaw].
pub struct AimInput {
    pub sensitivity: Reference<dyn ConVar>,
    pub pitch: Reference<dyn ConVar>,
    pub yaw: Reference<dyn ConVar>,
    pub recoil_scale: Reference<dyn ConVar>,
}

pub struct AimSnapshot {
    pub pawn: u32,
    pub weapon: u32,
    pub shots: i32,
    pub eye: Vector3<f32>,
    pub view: [f32; 2],
    pub recoil: [f32; 2],
    pub recoil_source: &'static str,
    pub degrees_per_count: [f32; 2],
}

impl AimInput {
    pub fn read(&self, ctx: &crate::UpdateContext) -> anyhow::Result<Option<AimSnapshot>> {
        let memory = ctx.states.resolve::<StateCS2Memory>(())?;
        let entities = ctx.states.resolve::<StateEntityList>(())?;
        let controller = ctx.states.resolve::<StateLocalPlayerController>(())?;
        let Some(controller) = controller.instance.value_reference(memory.view_arc()) else {
            return Ok(None);
        };
        let handle = controller.m_hPlayerPawn()?;
        let Some(pawn) = entities.entity_from_handle(&handle) else {
            return Ok(None);
        };
        let Some(pawn) = pawn.value_reference(memory.view_arc()) else {
            return Ok(None);
        };
        if pawn.m_iHealth()? <= 0 {
            return Ok(None);
        }
        let Some(services) = pawn.m_pWeaponServices()?.value_reference(memory.view_arc()) else {
            return Ok(None);
        };
        let weapon = services.m_hActiveWeapon()?;
        if !weapon.is_valid() {
            return Ok(None);
        }
        let origin = pawn
            .m_pGameSceneNode()?
            .value_reference(memory.view_arc())
            .context("missing local scene node")?
            .m_vecAbsOrigin()?;
        let offset = pawn.m_vecViewOffset()?;
        let eye = Vector3::from_row_slice(&origin)
            + Vector3::new(offset.m_vecX()?, offset.m_vecY()?, offset.m_vecZ()?);
        let view = pawn.m_angEyeAngles()?;
        let punch_sample = read_aim_punch(&memory, pawn.reference_address())?;
        let punch = punch_sample.angle;
        let scale = self.recoil_scale.fl_value()?;
        let sensitivity = self.sensitivity.fl_value()? * pawn.m_flFOVSensitivityAdjust()?;
        let degrees_per_count = [
            self.pitch.fl_value()? * sensitivity,
            self.yaw.fl_value()? * sensitivity,
        ];
        if !eye.iter().all(|v| v.is_finite())
            || !view[..2]
                .iter()
                .chain(punch[..2].iter())
                .all(|v| v.is_finite())
            || !scale.is_finite()
            || scale < 0.0
            || !degrees_per_count
                .iter()
                .all(|v| v.is_finite() && v.abs() > 0.000001)
        {
            return Ok(None);
        }
        Ok(Some(AimSnapshot {
            pawn: handle.value,
            weapon: weapon.value,
            shots: pawn.m_iShotsFired()?,
            eye,
            view: [view[0], view[1]],
            recoil: [punch[0] * scale, punch[1] * scale],
            recoil_source: punch_sample.source,
            degrees_per_count,
        }))
    }
}

pub fn wrap_angle(angle: f32) -> f32 {
    (angle + 180.0).rem_euclid(360.0) - 180.0
}

pub fn direction(angles: [f32; 2]) -> Vector3<f32> {
    let (pitch_sin, pitch_cos) = angles[0].to_radians().sin_cos();
    let (yaw_sin, yaw_cos) = angles[1].to_radians().sin_cos();
    Vector3::new(pitch_cos * yaw_cos, pitch_cos * yaw_sin, -pitch_sin)
}

/// Error from the recoil-adjusted shot cone center, before random spread.
pub fn shot_error(
    eye: Vector3<f32>,
    target: Vector3<f32>,
    view: [f32; 2],
    recoil: [f32; 2],
) -> Option<[f32; 2]> {
    let delta = target - eye;
    if !delta.iter().all(|v| v.is_finite()) || delta.norm_squared() < 0.000001 {
        return None;
    }
    let desired = [
        (-delta.z).atan2(delta.x.hypot(delta.y)).to_degrees(),
        delta.y.atan2(delta.x).to_degrees(),
    ];
    Some([
        wrap_angle(desired[0] - view[0] - recoil[0]),
        wrap_angle(desired[1] - view[1] - recoil[1]),
    ])
}

pub fn mouse_counts(angles: [f32; 2], degrees_per_count: [f32; 2]) -> [f32; 2] {
    // Positive mouse X turns right (negative yaw); positive mouse Y looks down.
    [
        -angles[1] / degrees_per_count[1],
        angles[0] / degrees_per_count[0],
    ]
}

/// Smooth target acquisition while compensating newly observed recoil immediately.
/// `error` already includes current recoil, so subtract only the unsmoothed share.
pub fn aim_correction(error: [f32; 2], recoil_delta: [f32; 2], smooth: [f32; 2]) -> [f32; 2] {
    std::array::from_fn(|axis| {
        let factor = smooth[axis].max(1.0);
        error[axis] / factor - recoil_delta[axis] * (1.0 - 1.0 / factor)
    })
}

#[derive(Default)]
pub struct RecoilHistory {
    previous: Option<(u32, u32, i32, [f32; 2])>,
}

impl RecoilHistory {
    pub fn reset(&mut self) {
        self.previous = None;
    }

    /// Also sample while aiming, so standalone RCS never replays an existing spray.
    pub fn sample(&mut self, pawn: u32, weapon: u32, shots: i32, recoil: [f32; 2]) -> [f32; 2] {
        let previous = self.previous.replace((pawn, weapon, shots, recoil));
        match previous {
            Some((old_pawn, old_weapon, old_shots, old))
                if pawn == old_pawn && weapon == old_weapon && shots > 0 && shots >= old_shots =>
            {
                [recoil[0] - old[0], recoil[1] - old[1]]
            }
            _ => [0.0; 2],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_does_not_delay_recoil_or_apply_it_twice() {
        // Previously aligned shot, followed by upward kick and rightward yaw.
        let delta = [-2.0, 1.0];
        for smooth in [1.0, 10.0, 50.0] {
            let correction = aim_correction([2.0, -1.0], delta, [smooth; 2]);
            assert!((correction[0] - 2.0).abs() < 0.0001);
            assert!((correction[1] + 1.0).abs() < 0.0001);
        }
        assert_eq!(aim_correction([10.0, 5.0], [0.0; 2], [10.0; 2]), [1.0, 0.5]);
    }

    #[test]
    fn shot_line_can_hit_target_away_from_crosshair() {
        let recoil = [-4.0, 2.0];
        let target = direction(recoil) * 1000.0;
        let error = shot_error(Vector3::zeros(), target, [0.0; 2], recoil).unwrap();
        assert!(error.iter().all(|v| v.abs() < 0.0001));
        let crosshair_error = shot_error(Vector3::zeros(), target, [0.0; 2], [0.0; 2]).unwrap();
        assert!(crosshair_error[0].abs() > 3.9);
    }

    #[test]
    fn recoil_correction_moves_down_and_scales_with_sensitivity() {
        let error = shot_error(
            Vector3::zeros(),
            Vector3::new(1000.0, 0.0, 0.0),
            [0.0; 2],
            [-2.0, 1.0],
        )
        .unwrap();
        let counts = mouse_counts(error, [0.022; 2]);
        assert!(counts[0] > 0.0 && counts[1] > 0.0);
        assert_eq!(
            mouse_counts(error, [0.044; 2]),
            [counts[0] / 2.0, counts[1] / 2.0]
        );
    }

    #[test]
    fn yaw_crossing_wraps_to_shortest_turn() {
        let error = shot_error(
            Vector3::zeros(),
            direction([0.0, -179.0]),
            [0.0, 179.0],
            [0.0; 2],
        )
        .unwrap();
        assert!((error[1] - 2.0).abs() < 0.0001);
    }

    #[test]
    fn history_does_not_repeat_recoil_or_replay_after_reset() {
        let mut history = RecoilHistory::default();
        assert_eq!(history.sample(1, 2, 2, [-2.0, 1.0]), [0.0; 2]);
        assert_eq!(history.sample(1, 2, 3, [-3.0, 1.5]), [-1.0, 0.5]);
        assert_eq!(history.sample(1, 2, 3, [-3.0, 1.5]), [0.0; 2]);
        assert_eq!(history.sample(1, 2, 0, [0.0; 2]), [0.0; 2]);
        assert_eq!(history.sample(1, 3, 4, [-4.0, 2.0]), [0.0; 2]);
        assert_eq!(history.sample(2, 3, 4, [-5.0, 3.0]), [0.0; 2]);
        history.reset();
        assert_eq!(history.sample(2, 3, 5, [-6.0, 3.5]), [0.0; 2]);
    }
}
