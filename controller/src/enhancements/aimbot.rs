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
    is_paused_by_settings: bool,
    boundary_enforced: bool,
}

impl AimBot {
    pub fn new() -> Self {
        Self {
            toggle: KeyToggle::new(),
            last_target_entity_id: None,
            last_smoothing_time: Instant::now(),
            current_target_position: None,
            is_paused_by_settings: false,
            boundary_enforced: false,
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
            let world_distance = (target_position - local_position).norm();

            // Prefer closer targets to crosshair
            if screen_distance < best_distance {
                best_distance = screen_distance;
                best_target = Some(AimTarget {
                    entity_id: entity_identity.handle::<()>()?.get_entity_index(),
                    target_position,
                    screen_position: [screen_pos.x, screen_pos.y],
                    distance_to_crosshair: screen_distance,
                    world_distance,
                });
            }
        }

        Ok(best_target)
    }

    fn calculate_smooth_movement(
        &mut self,
        target_screen_pos: [f32; 2],
        screen_center: [f32; 2],
        settings: &AppSettings,
        world_distance: f32,
    ) -> (i32, i32) {
        let delta_x = target_screen_pos[0] - screen_center[0];
        let delta_y = target_screen_pos[1] - screen_center[1];

        // Apply smoothing with optional distance scaling
        let mut smooth_factor_x = settings.aimbot_smoothness_x.max(1.0);
        let mut smooth_factor_y = settings.aimbot_smoothness_y.max(1.0);

        if settings.aimbot_distance_scaling {
            // Increase smoothing for longer distances (more gradual movement)
            let distance_factor = (world_distance / 1500.0).clamp(1.0, 2.5);
            smooth_factor_x *= distance_factor;
            smooth_factor_y *= distance_factor;
        }

        let mouse_move_x = (delta_x / smooth_factor_x) as i32;
        let mouse_move_y = (delta_y / smooth_factor_y) as i32;

        (mouse_move_x, mouse_move_y)
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
            self.is_paused_by_settings = true;
            return Ok(());
        } else {
            self.is_paused_by_settings = false;
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

        // Calculate distance-based tolerance
        let movement_tolerance = if settings.aimbot_distance_scaling {
            // Scale tolerance based on distance - farther targets need more precision (smaller tolerance)
            let base_tolerance = settings.aimbot_lock_strength;
            let distance_factor = (1000.0 / target.world_distance.max(100.0)).clamp(0.3, 2.0); // Inverse scaling
            base_tolerance * distance_factor
        } else {
            settings.aimbot_lock_strength
        };

        // Reset boundary enforcement flag
        self.boundary_enforced = false;

        // Boundary enforcement - prevent movement outside tolerance circle
        if target.distance_to_crosshair > movement_tolerance {
            self.boundary_enforced = true;
            // Calculate the direction from target to current crosshair position
            let delta_x = screen_center[0] - target.screen_position[0];
            let delta_y = screen_center[1] - target.screen_position[1];
            
            // Normalize the direction
            let distance = (delta_x * delta_x + delta_y * delta_y).sqrt();
            if distance > 0.0 {
                let norm_x = delta_x / distance;
                let norm_y = delta_y / distance;
                
                // Calculate the boundary position (exactly at tolerance distance)
                let boundary_x = target.screen_position[0] + norm_x * movement_tolerance;
                let boundary_y = target.screen_position[1] + norm_y * movement_tolerance;
                
                // Calculate movement needed to reach boundary
                let corrective_x = boundary_x - screen_center[0];
                let corrective_y = boundary_y - screen_center[1];
                
                let (mouse_x, mouse_y) = if settings.aimbot_strict_boundary {
                    // Strict mode: Move directly to boundary with minimal smoothing
                    let min_smoothing = 2.0;
                    ((corrective_x / min_smoothing) as i32, (corrective_y / min_smoothing) as i32)
                } else {
                    // Normal mode: Apply regular smoothing
                    self.calculate_smooth_movement(
                        [boundary_x, boundary_y],
                        screen_center,
                        &settings,
                        target.world_distance,
                    )
                };

                if mouse_x != 0 || mouse_y != 0 {
                    let mouse_state = MouseState {
                        last_x: mouse_x,
                        last_y: mouse_y,
                        ..Default::default()
                    };

                    ctx.cs2.send_mouse_state(&[mouse_state])?;
                    self.last_smoothing_time = Instant::now();
                }
            }
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
        let view = states.resolve::<ViewController>(())?;

        // Always draw FOV circle when enabled (regardless of aimbot toggle state)
        if settings.aimbot_show_fov {
            let draw_list = ui.get_window_draw_list();
            let screen_center = [
                view.screen_bounds.x / 2.0,
                view.screen_bounds.y / 2.0,
            ];

            draw_list.add_circle(
                screen_center,
                settings.aimbot_fov_radius,
                [1.0, 1.0, 1.0, 0.3],
            )
            .thickness(1.0)
            .build();
        }

        if !self.toggle.enabled {
            return Ok(());
        }

        // Draw tolerance circle to show the "dead zone"
        if settings.aimbot_show_debug {
            if let Some(target_pos) = &self.current_target_position {
                if let Some(screen_pos) = view.world_to_screen(target_pos, false) {
                    let draw_list = ui.get_window_draw_list();
                    let pos = [screen_pos.x, screen_pos.y];
                    
                    let tolerance = if settings.aimbot_distance_scaling {
                        let distance = if let Some(camera_pos) = view.get_camera_world_position() {
                            (target_pos - camera_pos).norm()
                        } else {
                            1000.0
                        };
                        // Inverse scaling - smaller tolerance for farther targets
                        let distance_factor = (1000.0 / distance.max(100.0)).clamp(0.3, 2.0);
                        settings.aimbot_lock_strength * distance_factor
                    } else {
                        settings.aimbot_lock_strength
                    };

                    // Make circle more prominent when boundary is being enforced
                    let (color, thickness) = if self.boundary_enforced {
                        ([1.0, 0.5, 0.0, 0.6], 2.0) // Orange, thicker when enforcing
                    } else {
                        ([0.0, 1.0, 0.0, 0.3], 1.0) // Green, normal when not enforcing
                    };

                    draw_list.add_circle(
                        pos,
                        tolerance,
                        color,
                    )
                    .thickness(thickness)
                    .build();
                }
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

        let view = states.resolve::<ViewController>(())?;

        ui.window(obfstr!("Aimbot Debug"))
            .size([300.0, 200.0], Condition::FirstUseEver)
            .build(|| {
                ui.text(format!("Aimbot Enabled: {}", self.toggle.enabled));
                ui.text(format!("Status: {}", if self.is_paused_by_settings { "Paused (Settings Open)" } else { "Running" }));
                
                if let Some(entity_id) = self.last_target_entity_id {
                    ui.text(format!("Target Entity ID: {}", entity_id));
                } else {
                    ui.text("No Target");
                }
                
                if let Some(pos) = &self.current_target_position {
                    ui.text(format!("Target Position: [{:.1}, {:.1}, {:.1}]", pos.x, pos.y, pos.z));
                }
                
                let time_since_move = self.last_smoothing_time.elapsed().as_millis();
                ui.text(format!("Last Mouse Move: {}ms ago", time_since_move));
                ui.text(format!("Boundary Enforced: {}", if self.boundary_enforced { "YES" } else { "No" }));
                
                ui.separator();
                ui.text(format!("Bone Target: {}", settings.aimbot_bone_target.display_name()));
                ui.text(format!("FOV Radius: {:.0}px", settings.aimbot_fov_radius));
                ui.text(format!("Smoothness: X={:.1}, Y={:.1}", settings.aimbot_smoothness_x, settings.aimbot_smoothness_y));
                ui.text(format!("Lock Strength: {:.1}", settings.aimbot_lock_strength));
                ui.text(format!("Distance Scaling: {}", if settings.aimbot_distance_scaling { "On" } else { "Off" }));
                ui.text(format!("Strict Boundary: {}", if settings.aimbot_strict_boundary { "On" } else { "Off" }));
                
                if let Some(pos) = &self.current_target_position {
                    if let Some(camera_pos) = view.get_camera_world_position() {
                        let distance = (pos - camera_pos).norm();
                        ui.text(format!("Target Distance: {:.0} units", distance));
                        
                        if settings.aimbot_distance_scaling {
                            // Use inverse scaling - smaller tolerance for farther targets
                            let distance_factor = (1000.0 / distance.max(100.0)).clamp(0.3, 2.0);
                            let effective_tolerance = settings.aimbot_lock_strength * distance_factor;
                            ui.text(format!("Effective Tolerance: {:.1}px", effective_tolerance));
                        }
                    }
                }
            });

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct AimTarget {
    entity_id: u32,
    target_position: Vector3<f32>,
    screen_position: [f32; 2],
    distance_to_crosshair: f32,
    world_distance: f32,
} 