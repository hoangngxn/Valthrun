use std::time::Instant;

use anyhow::Context;
use cs2::{
    BoneFlags,
    ClassNameCache,
    CS2Model,
    MouseState,
    StateCS2Memory,
    StateEntityList,
    StateLocalPlayerController,
    StatePawnInfo,
    StatePawnModelInfo,
    LocalCameraControllerTarget,
    PlayerPawnState,
    CEntityIdentityEx,
};
use cs2_schema_generated::cs2::client::C_BasePlayerPawn;
use imgui::Condition;
use nalgebra::Vector3;
use obfstr::obfstr;
use overlay::UnicodeTextRenderer;
use utils_state::StateRegistry;

use super::Enhancement;
use crate::{
    settings::{AppSettings, BoneTarget},
    view::{
        KeyToggle,
        ViewController,
    },
    UpdateContext,
};

pub struct AimBot {
    toggle: KeyToggle,
    last_target_entity_id: Option<u32>,
    last_smoothing_time: Instant,
    current_target_position: Option<Vector3<f32>>,
    last_rcs_values: Option<(f32, f32)>,
}

impl AimBot {
    pub fn new() -> Self {
        Self {
            toggle: KeyToggle::new(),
            last_target_entity_id: None,
            last_smoothing_time: Instant::now(),
            current_target_position: None,
            last_rcs_values: None,
        }
    }

    fn get_bone_position(
        &self,
        bone_target: BoneTarget,
        pawn_info: &StatePawnInfo,
        pawn_model: &StatePawnModelInfo,
        model: &CS2Model,
        view: &ViewController,
        screen_center: [f32; 2],
    ) -> Option<Vector3<f32>> {
        match bone_target {
            BoneTarget::Head => {
                // Use standard head position offset
                Some(pawn_info.position + Vector3::new(0.0, 0.0, 64.0))
            }
            BoneTarget::Neck => {
                // Find neck bone or approximate position
                self.find_bone_by_name(model, &["neck", "spine_3"], pawn_model)
                    .or_else(|| Some(pawn_info.position + Vector3::new(0.0, 0.0, 60.0)))
            }
            BoneTarget::Chest => {
                // Find chest/spine bones
                self.find_bone_by_name(model, &["spine_2", "chest", "spine_1"], pawn_model)
                    .or_else(|| Some(pawn_info.position + Vector3::new(0.0, 0.0, 40.0)))
            }
            BoneTarget::Stomach => {
                // Find stomach/pelvis bones
                self.find_bone_by_name(model, &["pelvis", "spine_0"], pawn_model)
                    .or_else(|| Some(pawn_info.position + Vector3::new(0.0, 0.0, 20.0)))
            }
            BoneTarget::Closest => {
                // Find bone closest to crosshair
                self.find_closest_bone(pawn_model, model, view, screen_center)
            }
        }
    }

    fn find_bone_by_name(
        &self,
        model: &CS2Model,
        bone_names: &[&str],
        pawn_model: &StatePawnModelInfo,
    ) -> Option<Vector3<f32>> {
        for bone_name in bone_names {
            for (index, bone) in model.bones.iter().enumerate() {
                if index >= pawn_model.bone_states.len() {
                    continue;
                }
                
                let bone_name_lower = bone.name.to_lowercase();
                if bone_name_lower.contains(&bone_name.to_lowercase()) {
                    return Some(pawn_model.bone_states[index].position);
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

    fn find_best_target(&self, ctx: &UpdateContext) -> anyhow::Result<Option<AimTarget>> {
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
        let local_pawn_handle = local_player_controller.m_hPlayerPawn()?;
        let local_pawn = entities
            .entity_from_handle(&local_pawn_handle)
            .context("entity from handle failed")?
            .value_reference(memory.view_arc())
            .context("local player pawn")?;

        let local_position = Vector3::from_row_slice(&local_pawn.m_vOldOrigin()?);
        
        let view_target = ctx.states.resolve::<LocalCameraControllerTarget>(())?;
        let view_target_entity_id = match &view_target.target_entity_id {
            Some(value) => *value,
            None => return Ok(None),
        };

        let screen_center = [
            view.screen_bounds.x / 2.0,
            view.screen_bounds.y / 2.0,
        ];

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
                &pawn_info,
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

            // Calculate world distance for scaling
            let _world_distance = (target_position - local_position).norm();

            // Prefer closer targets to crosshair
            if screen_distance < best_distance {
                best_distance = screen_distance;
                best_target = Some(AimTarget {
                    entity_id: entity_identity.handle::<()>()?.get_entity_index(),
                    target_position,
                    screen_position: [screen_pos.x, screen_pos.y],
                });
            }
        }

        Ok(best_target)
    }

    fn calculate_smooth_movement_with_rcs(
        &mut self,
        target_screen_pos: [f32; 2],
        screen_center: [f32; 2],
        settings: &AppSettings,
        rcs_compensation: Option<(f32, f32)>,
    ) -> (i32, i32) {
        // Calculate deltas exactly like C++
        let mut delta_x = target_screen_pos[0] - screen_center[0];
        let mut delta_y = target_screen_pos[1] - screen_center[1];

        // Apply RCS compensation exactly like C++: deltaX -= rcsX; deltaY -= rcsY
        if let Some((rcs_x, rcs_y)) = rcs_compensation {
            delta_x -= rcs_x;
            delta_y -= rcs_y;
        }

        // Apply smoothing (simpler logic without distance scaling)
        let smooth_factor_x = settings.aimbot_smoothness_x.max(1.0);
        let smooth_factor_y = settings.aimbot_smoothness_y.max(1.0);

        let mouse_move_x = (delta_x / smooth_factor_x) as i32;
        let mouse_move_y = (delta_y / smooth_factor_y) as i32;

        (mouse_move_x, mouse_move_y)
    }

    fn calculate_rcs_compensation(
        &self,
        ctx: &UpdateContext,
    ) -> anyhow::Result<Option<(f32, f32)>> {
        let settings = ctx.states.resolve::<AppSettings>(())?;
        if !settings.aimbot_rcs_enabled {
            return Ok(None);
        }

        let memory = ctx.states.resolve::<StateCS2Memory>(())?;
        let entities = ctx.states.resolve::<StateEntityList>(())?;
        let local_controller = ctx.states.resolve::<StateLocalPlayerController>(())?;
        
        let local_pawn_handle = match local_controller.instance.value_reference(memory.view_arc()) {
            Some(local_controller) => local_controller.m_hPlayerPawn()?,
            None => return Ok(None),
        };

        let local_pawn = entities
            .entity_from_handle(&local_pawn_handle)
            .context("missing local player pawn")?
            .value_reference(memory.view_arc())
            .context("nullptr")?;

        let shots_fired = local_pawn.m_iShotsFired()?;
        if shots_fired <= 1 {
            return Ok(None);
        }

        // Get punch angle data (exact C++ logic)
        let punch_angle = nalgebra::Vector3::from_row_slice(&local_pawn.m_aimPunchAngle()?[0..3]);
        let punch_angle_vel = nalgebra::Vector3::from_row_slice(&local_pawn.m_aimPunchAngleVel()?[0..3]);
        let _punch_tick_base = local_pawn.m_aimPunchTickBase()?;
        let punch_tick_fraction = local_pawn.m_aimPunchTickFraction()?;
        
        // Calculate RCS exactly like C++: punchAngle * recoilControlSystem strength
        let mut rcs_x = punch_angle.y * settings.aimbot_rcs_x; // Note: X uses Y component (yaw)
        let mut rcs_y = punch_angle.x * settings.aimbot_rcs_y; // Note: Y uses X component (pitch)

        // Add velocity compensation exactly like C++: punchAngleVel * punchTickFraction * 0.1f
        rcs_x += punch_angle_vel.y * punch_tick_fraction * 0.1;
        rcs_y += punch_angle_vel.x * punch_tick_fraction * 0.1;

        // Note: Punch cache access requires more complex memory handling
        // The core RCS logic above provides most of the recoil compensation benefit
        // Cache smoothing can be added later if needed

        Ok(Some((rcs_x, rcs_y)))
    }
}

impl Enhancement for AimBot {
    fn update(&mut self, ctx: &UpdateContext) -> anyhow::Result<()> {
        let settings = ctx.states.resolve::<AppSettings>(())?;
        
        if self.toggle.update(
            &settings.aimbot_mode,
            ctx.input,
            &settings.key_aimbot,
        ) {
            ctx.cs2.add_metrics_record(
                obfstr!("feature-aimbot-toggle"),
                &format!(
                    "enabled: {}, mode: {:?}",
                    self.toggle.enabled, settings.aimbot_mode
                ),
            );
        }

        // Pause aimbot when settings menu is open to prevent interference during configuration
        if ctx.settings_visible {
            return Ok(());
        }

        if !self.toggle.enabled {
            return Ok(());
        }

        let view = ctx.states.resolve::<ViewController>(())?;
        let screen_center = [
            view.screen_bounds.x / 2.0,
            view.screen_bounds.y / 2.0,
        ];

        // Find best target
        let target = self.find_best_target(ctx)?;
        let Some(target) = target else {
            self.last_target_entity_id = None;
            self.current_target_position = None;
            return Ok(());
        };

        // Update target tracking
        self.last_target_entity_id = Some(target.entity_id);
        self.current_target_position = Some(target.target_position);

        // Calculate RCS compensation
        let rcs_compensation = self.calculate_rcs_compensation(ctx)?;
        self.last_rcs_values = rcs_compensation;

        // Simple aimbot - smoothly aim towards target with RCS compensation
        let (mouse_x, mouse_y) = self.calculate_smooth_movement_with_rcs(
            target.screen_position,
            screen_center,
            &settings,
            rcs_compensation,
        );

        // Only move if the movement is significant enough to avoid micro-movements
        let movement_threshold = 1;
        if mouse_x.abs() >= movement_threshold || mouse_y.abs() >= movement_threshold {
            let mouse_state = MouseState {
                last_x: mouse_x,
                last_y: mouse_y,
                ..Default::default()
            };

            ctx.cs2.send_mouse_state(&[mouse_state])?;
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
            let view = states.resolve::<ViewController>(())?;
            let draw_list = ui.get_window_draw_list();
            let screen_center = [
                view.screen_bounds.x / 2.0,
                view.screen_bounds.y / 2.0,
            ];

            // Simple FOV circle drawing with error handling
            if screen_center[0] > 0.0 && screen_center[1] > 0.0 && settings.aimbot_fov_radius > 0.0 {
                draw_list
                    .add_circle(screen_center, settings.aimbot_fov_radius, [1.0, 1.0, 1.0, 0.3])
                    .thickness(1.0)
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
            .size([280.0, 180.0], Condition::FirstUseEver)
            .build(|| {
                ui.text(format!("Aimbot Active: {}", self.toggle.enabled));
                
                if let Some(entity_id) = self.last_target_entity_id {
                    ui.text(format!("Target ID: {}", entity_id));
                } else {
                    ui.text("Target: None");
                }
                
                ui.separator();
                ui.text(format!("FOV: {:.0}px", settings.aimbot_fov_radius));
                ui.text(format!("Smoothness: {:.1}, {:.1}", 
                    settings.aimbot_smoothness_x, settings.aimbot_smoothness_y));
                
                ui.separator();
                if settings.aimbot_rcs_enabled {
                    ui.text(format!("RCS: {:.2}, {:.2}", 
                        settings.aimbot_rcs_x, settings.aimbot_rcs_y));
                    
                    if let Some((rcs_x, rcs_y)) = self.last_rcs_values {
                        ui.text(format!("Active RCS: {:.2}, {:.2}", rcs_x, rcs_y));
                    } else {
                        ui.text("RCS: Inactive");
                    }
                } else {
                    ui.text("RCS: Disabled");
                }
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
    screen_position: [f32; 2],
} 