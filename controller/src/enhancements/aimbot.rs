use std::time::Instant;

use cs2::{
    BoneFlags,
    CEntityIdentityEx,
    CS2Model,
    ClassNameCache,
    LocalCameraControllerTarget,
    MouseState,
    PlayerPawnState,
    StateCS2Memory,
    StateEntityList,
    StateLocalPlayerController,
    StatePawnInfo,
    StatePawnModelInfo,
};
use imgui::Condition;
use nalgebra::Vector3;
use obfstr::obfstr;
use overlay::UnicodeTextRenderer;
use utils_state::StateRegistry;

use super::{
    aim::{
        aim_correction,
        direction,
        mouse_counts,
        shot_error,
        AimInput,
        RecoilHistory,
    },
    Enhancement,
};
use crate::{
    settings::{
        AppSettings,
        BoneTarget,
    },
    view::{
        KeyToggle,
        ViewController,
    },
    UpdateContext,
};

pub struct AimBot {
    input: AimInput,
    recoil_history: RecoilHistory,
    shot_screen_position: Option<[f32; 2]>,
    local_identity: Option<(u32, u32)>,
    recoil_source: Option<&'static str>,
    toggle: KeyToggle,
    last_target_entity_id: Option<u32>,
    last_smoothing_time: Instant,
    current_target_position: Option<Vector3<f32>>,
    last_rcs_values: Option<(f32, f32)>,
    movement_remainder: [f32; 2],
}

impl AimBot {
    pub fn new(input: AimInput) -> Self {
        Self {
            input,
            recoil_history: RecoilHistory::default(),
            shot_screen_position: None,
            local_identity: None,
            recoil_source: None,
            toggle: KeyToggle::new(),
            last_target_entity_id: None,
            last_smoothing_time: Instant::now(),
            current_target_position: None,
            last_rcs_values: None,
            movement_remainder: [0.0; 2],
        }
    }

    fn reset_tracking(&mut self) {
        self.last_target_entity_id = None;
        self.current_target_position = None;
        self.last_rcs_values = None;
        self.movement_remainder = [0.0; 2];
    }

    fn reset(&mut self) {
        self.reset_tracking();
        self.recoil_history.reset();
        self.shot_screen_position = None;
        self.local_identity = None;
        self.recoil_source = None;
    }

    fn get_bone_position(
        &self,
        bone_target: BoneTarget,
        pawn_model: &StatePawnModelInfo,
        model: &CS2Model,
        view: &ViewController,
        screen_center: [f32; 2],
    ) -> Option<Vector3<f32>> {
        match bone_target {
            BoneTarget::Head => Self::find_bone_by_name(model, &["head_0", "head"], pawn_model),
            BoneTarget::Neck => {
                // Skip unavailable bones instead of guessing a standing pose.
                Self::find_bone_by_name(model, &["neck_0", "neck", "spine_3"], pawn_model)
            }
            BoneTarget::Chest => {
                // Find chest/spine bones
                Self::find_bone_by_name(model, &["spine_2", "chest", "spine_1"], pawn_model)
            }
            BoneTarget::Stomach => {
                // Find stomach/pelvis bones
                Self::find_bone_by_name(model, &["pelvis", "spine_0"], pawn_model)
            }
            BoneTarget::Closest => {
                // Find bone closest to crosshair
                self.find_closest_bone(pawn_model, model, view, screen_center)
            }
        }
    }

    fn find_bone_by_name(
        model: &CS2Model,
        bone_names: &[&str],
        pawn_model: &StatePawnModelInfo,
    ) -> Option<Vector3<f32>> {
        for bone_name in bone_names {
            for (index, bone) in model.bones.iter().enumerate() {
                if index >= pawn_model.bone_states.len() {
                    continue;
                }

                // Exact names avoid selecting similarly named helper bones.
                if bone.name.eq_ignore_ascii_case(bone_name) {
                    let position = pawn_model.bone_states[index].position;
                    if position.iter().all(|value| value.is_finite()) {
                        return Some(position);
                    }
                }
            }
        }
        None
    }

    fn find_closest_bone(
        &self,
        pawn_model: &StatePawnModelInfo,
        model: &CS2Model,
        view: &ViewController,
        screen_center: [f32; 2],
    ) -> Option<Vector3<f32>> {
        let mut closest_distance = f32::MAX;
        let mut closest_position = None;

        for (index, bone) in model.bones.iter().enumerate() {
            if index >= pawn_model.bone_states.len() {
                continue;
            }

            // Only consider hitbox bones
            if (bone.flags & BoneFlags::FlagHitbox as u32) == 0 {
                continue;
            }

            let bone_position = pawn_model.bone_states[index].position;
            if !bone_position.iter().all(|value| value.is_finite()) {
                continue;
            }

            // Project to screen and check distance to crosshair
            if let Some(screen_pos) = view.world_to_screen(&bone_position, false) {
                let distance = ((screen_pos.x - screen_center[0]).powi(2)
                    + (screen_pos.y - screen_center[1]).powi(2))
                .sqrt();

                if distance < closest_distance {
                    closest_distance = distance;
                    closest_position = Some(bone_position);
                }
            }
        }

        closest_position
    }

    fn find_best_target(
        &self,
        ctx: &UpdateContext,
        screen_center: [f32; 2],
    ) -> anyhow::Result<Option<AimTarget>> {
        let memory = ctx.states.resolve::<StateCS2Memory>(())?;
        let entities = ctx.states.resolve::<StateEntityList>(())?;
        let class_name_cache = ctx.states.resolve::<ClassNameCache>(())?;
        let settings = ctx.states.resolve::<AppSettings>(())?;
        let view = ctx.states.resolve::<ViewController>(())?;

        let local_player_controller = ctx.states.resolve::<StateLocalPlayerController>(())?;
        let Some(local_player_controller) = local_player_controller
            .instance
            .value_reference(memory.view_arc())
        else {
            return Ok(None);
        };

        let local_team_id = local_player_controller.m_iPendingTeamNum()?;

        let view_target = ctx.states.resolve::<LocalCameraControllerTarget>(())?;
        let view_target_entity_id = match &view_target.target_entity_id {
            Some(value) => *value,
            None => return Ok(None),
        };

        let mut best_target: Option<AimTarget> = None;
        let mut best_distance = f32::MAX;

        for entity_identity in entities.entities() {
            if entity_identity.handle::<()>()?.get_entity_index() == view_target_entity_id {
                continue;
            }

            let entity_class = class_name_cache.lookup(&entity_identity.entity_class_info()?)?;
            if !entity_class
                .map(|name| *name == "C_CSPlayerPawn")
                .unwrap_or(false)
            {
                continue;
            }

            let pawn_state = ctx
                .states
                .resolve::<PlayerPawnState>(entity_identity.handle()?)?;
            if *pawn_state != PlayerPawnState::Alive {
                continue;
            }

            let pawn_info = ctx
                .states
                .resolve::<StatePawnInfo>(entity_identity.handle()?)?;

            if pawn_info.player_health <= 0 {
                continue;
            }

            // Team check
            if settings.aimbot_team_check {
                if pawn_info.team_id == local_team_id {
                    continue;
                }
            }

            let pawn_model = ctx
                .states
                .resolve::<StatePawnModelInfo>(entity_identity.handle()?)?;

            let model = ctx.states.resolve::<CS2Model>(pawn_model.model_address)?;

            // Get target bone position based on settings
            let target_position = match self.get_bone_position(
                settings.aimbot_bone_target,
                &pawn_model,
                &model,
                &view,
                screen_center,
            ) {
                Some(pos) => pos,
                None => continue, // Skip if no valid bone position found
            };

            // Check if target is in FOV
            let screen_pos = view.world_to_screen(&target_position, false);
            let Some(screen_pos) = screen_pos else {
                continue;
            };

            let screen_distance = ((screen_pos.x - screen_center[0]).powi(2)
                + (screen_pos.y - screen_center[1]).powi(2))
            .sqrt();

            if screen_distance > settings.aimbot_fov_radius {
                continue;
            }

            // Prefer closer targets to crosshair
            if screen_distance < best_distance {
                best_distance = screen_distance;
                best_target = Some(AimTarget {
                    entity_id: entity_identity.handle::<()>()?.get_entity_index(),
                    target_position,
                });
            }
        }

        Ok(best_target)
    }
}

impl Enhancement for AimBot {
    fn update(&mut self, ctx: &UpdateContext) -> anyhow::Result<()> {
        let settings = ctx.states.resolve::<AppSettings>(())?;
        if self
            .toggle
            .update(&settings.aimbot_mode, ctx.input, &settings.key_aimbot)
        {
            ctx.cs2.add_metrics_record(
                obfstr!("feature-aimbot-toggle"),
                &format!(
                    "enabled: {}, mode: {:?}",
                    self.toggle.enabled, settings.aimbot_mode
                ),
            );
        }
        if ctx.settings_visible || (!self.toggle.enabled && !settings.aim_assist_recoil) {
            self.reset();
            return Ok(());
        }
        let snapshot = match self.input.read(ctx) {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) => {
                self.reset();
                return Ok(());
            }
            Err(error) => {
                self.reset();
                return Err(error);
            }
        };
        let identity = (snapshot.pawn, snapshot.weapon);
        if self.local_identity != Some(identity) {
            self.reset();
            self.local_identity = Some(identity);
        }
        self.recoil_source = Some(snapshot.recoil_source);
        let recoil_delta = self.recoil_history.sample(
            snapshot.pawn,
            snapshot.weapon,
            snapshot.shots,
            snapshot.recoil,
        );
        let view = ctx.states.resolve::<ViewController>(())?;
        let shot_angles = [
            snapshot.view[0] + snapshot.recoil[0],
            snapshot.view[1] + snapshot.recoil[1],
        ];
        // Project a point far along the shot ray, not the screen-center crosshair.
        self.shot_screen_position = view
            .world_to_screen(&(snapshot.eye + direction(shot_angles) * 10000.0), true)
            .filter(|p| p.x.is_finite() && p.y.is_finite())
            .map(|p| [p.x, p.y]);
        let target = if self.toggle.enabled {
            match self.shot_screen_position {
                Some(origin) => self.find_best_target(ctx, origin)?,
                None => None,
            }
        } else {
            None
        };

        let angles = if let Some(target) = target {
            let continuing_target = self.last_target_entity_id == Some(target.entity_id);
            if !continuing_target {
                self.movement_remainder = [0.0; 2];
            }
            self.last_target_entity_id = Some(target.entity_id);
            self.current_target_position = Some(target.target_position);
            // The standalone helper also enables recoil-aware targeting. It has no
            // separate output while this controller owns a target.
            let strength = if settings.aimbot_rcs_enabled {
                [
                    settings.aimbot_rcs_y.clamp(0.0, 2.0),
                    settings.aimbot_rcs_x.clamp(0.0, 2.0),
                ]
            } else if settings.aim_assist_recoil {
                [1.0; 2]
            } else {
                [0.0; 2]
            };
            let recoil = [
                snapshot.recoil[0] * strength[0],
                snapshot.recoil[1] * strength[1],
            ];
            self.last_rcs_values = Some((recoil[1], recoil[0]));
            let Some(error) =
                shot_error(snapshot.eye, target.target_position, snapshot.view, recoil)
            else {
                self.reset_tracking();
                return Ok(());
            };
            let delta = if continuing_target {
                [recoil_delta[0] * strength[0], recoil_delta[1] * strength[1]]
            } else {
                [0.0; 2]
            };
            aim_correction(
                error,
                delta,
                [settings.aimbot_smoothness_y, settings.aimbot_smoothness_x],
            )
        } else {
            if self.last_target_entity_id.is_some() {
                self.reset_tracking();
            }
            if !settings.aim_assist_recoil
                || snapshot.shots <= 0
                || snapshot.shots as u32 <= settings.aim_assist_recoil_min_bullets
            {
                self.movement_remainder = [0.0; 2];
                self.last_rcs_values = None;
                return Ok(());
            }
            self.last_rcs_values = Some((recoil_delta[1], recoil_delta[0]));
            [-recoil_delta[0], -recoil_delta[1]]
        };
        let counts = mouse_counts(angles, snapshot.degrees_per_count);
        let mouse_x = accumulate_movement(counts[0], &mut self.movement_remainder[0]);
        let mouse_y = accumulate_movement(counts[1], &mut self.movement_remainder[1]);
        if mouse_x != 0 || mouse_y != 0 {
            ctx.cs2.send_mouse_state(&[MouseState {
                last_x: mouse_x,
                last_y: mouse_y,
                ..Default::default()
            }])?;
            self.last_smoothing_time = Instant::now();
        }
        Ok(())
    }
    fn update_settings(
        &mut self,
        _ui: &imgui::Ui,
        _settings: &mut AppSettings,
    ) -> anyhow::Result<bool> {
        // Settings are now handled in the dedicated "Aimbot" tab in the main UI
        Ok(false)
    }

    fn render(
        &self,
        states: &StateRegistry,
        ui: &imgui::Ui,
        _unicode_text: &UnicodeTextRenderer,
    ) -> anyhow::Result<()> {
        let settings = states.resolve::<AppSettings>(())?;

        // Only draw FOV circle when enabled AND aimbot is active
        if settings.aimbot_show_fov && self.toggle.enabled {
            let draw_list = ui.get_window_draw_list();
            let Some(screen_center) = self.shot_screen_position else {
                return Ok(());
            };

            // Simple FOV circle drawing with error handling
            if settings.aimbot_fov_radius > 0.0 {
                draw_list
                    .add_circle(
                        screen_center,
                        settings.aimbot_fov_radius,
                        [1.0, 1.0, 1.0, 0.3],
                    )
                    .thickness(1.0)
                    .build();
                draw_list
                    .add_circle(screen_center, 3.0, [1.0, 0.65, 0.1, 1.0])
                    .thickness(1.5)
                    .build();
            }
        }

        Ok(())
    }

    fn render_debug_window(
        &mut self,
        states: &StateRegistry,
        ui: &imgui::Ui,
        _unicode_text: &UnicodeTextRenderer,
    ) -> anyhow::Result<()> {
        let settings = states.resolve::<AppSettings>(())?;

        // Only show debug window if the setting is enabled
        if !settings.aimbot_show_debug {
            return Ok(());
        }

        // Simple debug window with minimal information to prevent freezing
        let mut show_debug = settings.aimbot_show_debug;
        ui.window(obfstr!("Aimbot Debug"))
            .opened(&mut show_debug)
            .size([320.0, 230.0], Condition::FirstUseEver)
            .build(|| {
                ui.text(format!("Aimbot Active: {}", self.toggle.enabled));

                if let Some(entity_id) = self.last_target_entity_id {
                    ui.text(format!("Target ID: {}", entity_id));
                } else {
                    ui.text("Target: None");
                }

                ui.separator();
                ui.text(format!("FOV: {:.0}px", settings.aimbot_fov_radius));
                ui.text(format!(
                    "Smoothness: {:.1}, {:.1}",
                    settings.aimbot_smoothness_x, settings.aimbot_smoothness_y
                ));

                ui.separator();
                if settings.aimbot_rcs_enabled || settings.aim_assist_recoil {
                    ui.text(format!(
                        "RCS: {:.2}, {:.2}",
                        settings.aimbot_rcs_x, settings.aimbot_rcs_y
                    ));

                    if let Some((rcs_x, rcs_y)) = self.last_rcs_values {
                        ui.text(format!("Recoil degrees: {:.2}, {:.2}", rcs_x, rcs_y));
                    } else {
                        ui.text("RCS: Inactive");
                    }
                } else {
                    ui.text("RCS: Disabled");
                }
                ui.text("Shot marker excludes random spread");
                ui.text(self.recoil_source.unwrap_or("Recoil source: inactive"));
            });

        // If user closed the debug window, update the setting
        if !show_debug {
            // We can't directly modify settings here, so we'll just note it
            // The user will need to turn it off in the main settings
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct AimTarget {
    entity_id: u32,
    target_position: Vector3<f32>,
}

// Preserve sub-count movement so smoothing can converge on small distant targets.
fn accumulate_movement(step: f32, remainder: &mut f32) -> i32 {
    if !step.is_finite() {
        *remainder = 0.0;
        return 0;
    }
    // Repeated reads can have zero recoil delta between game updates.
    if step == 0.0 {
        return 0;
    }
    if step.signum() != remainder.signum() {
        *remainder = 0.0;
    }
    let movement = step + *remainder;
    let whole = movement as i32;
    *remainder = movement - whole as f32;
    whole
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_target_uses_animated_bone_and_rejects_helper_names() {
        let model = CS2Model {
            bones: vec![
                cs2::Bone {
                    name: "head_helper".into(),
                    ..Default::default()
                },
                cs2::Bone {
                    name: "head_0".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let animated_head = Vector3::new(100.0, 200.0, 43.0);
        let pawn_model = StatePawnModelInfo {
            model_address: 0,
            bone_states: vec![
                cs2::BoneStateData {
                    position: Vector3::zeros(),
                },
                cs2::BoneStateData {
                    position: animated_head,
                },
            ],
        };
        assert_eq!(
            AimBot::find_bone_by_name(&model, &["head_0", "head"], &pawn_model),
            Some(animated_head)
        );
        assert_eq!(
            AimBot::find_bone_by_name(&model, &["neck"], &pawn_model),
            None
        );
    }

    #[test]
    fn small_movements_eventually_produce_input() {
        for step in [0.25, -0.25] {
            let mut remainder = 0.0;
            let total: i32 = (0..40)
                .map(|_| accumulate_movement(step, &mut remainder))
                .sum();
            assert_eq!(total, (step * 40.0) as i32);
        }
    }

    #[test]
    fn reversal_discards_stale_movement_but_idle_samples_preserve_precision() {
        let mut remainder = 0.75;
        assert_eq!(accumulate_movement(-0.25, &mut remainder), 0);
        assert_eq!(remainder, -0.25);
        assert_eq!(accumulate_movement(0.0, &mut remainder), 0);
        assert_eq!(remainder, -0.25);
    }
}
