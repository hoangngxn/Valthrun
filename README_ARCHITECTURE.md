# Valthrun CS2 Enhancer - Architecture Documentation

## Overview

Valthrun is a sophisticated CS2 (Counter-Strike 2) game enhancer/cheat that provides various gameplay enhancements through real-time memory reading, overlay rendering, and external radar functionality. The system is built in Rust and utilizes a modular architecture with multiple components working together.

## High-Level Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Controller    │    │    Overlay      │    │     Radar       │
│  (Main Logic)   │◄──►│   (Rendering)   │    │   (External)    │
└─────────────────┘    └─────────────────┘    └─────────────────┘
         │                       │                       │
         ▼                       ▼                       ▼
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   CS2 Library   │    │  Graphics APIs  │    │   Web Client    │
│ (Memory Access) │    │ (DX/VK/OpenGL)  │    │   (Browser)     │
└─────────────────┘    └─────────────────┘    └─────────────────┘
         │
         ▼
┌─────────────────┐
│  CS2 Schema     │
│ (Game Offsets)  │
└─────────────────┘
```

## Core Components

### 1. Controller (`controller/src/`)

The main application that orchestrates all game enhancements and provides the user interface.

#### Main Entry Point (`main.rs`)
- **Application Initialization**: Sets up logging, checks admin privileges, loads settings
- **CS2 Handle Creation**: Establishes connection to CS2 process using kernel driver interface
- **State Registry**: Centralized state management system for all game data
- **Enhancement Loading**: Initializes all enhancement modules (ESP, aimbot, trigger bot, etc.)
- **Overlay Integration**: Creates and manages the graphics overlay system
- **Main Loop**: Runs the update/render cycle at ~60 FPS

#### Enhancement System (`enhancements/`)

**Enhancement Trait**:
```rust
pub trait Enhancement {
    fn update(&mut self, ctx: &UpdateContext) -> anyhow::Result<()>;
    fn render(&self, states: &StateRegistry, ui: &imgui::Ui, unicode_text: &UnicodeTextRenderer) -> anyhow::Result<()>;
    fn update_settings(&mut self, ui: &imgui::Ui, settings: &mut AppSettings) -> anyhow::Result<bool>;
    fn render_debug_window(&mut self, states: &StateRegistry, ui: &imgui::Ui, unicode_text: &UnicodeTextRenderer) -> anyhow::Result<()>;
}
```

**Available Enhancements**:

1. **Player ESP** (`player/mod.rs`):
   - Shows enemy/teammate information through walls
   - Displays health, name, weapon, distance
   - Configurable team filtering and visual styles
   - Bone/skeleton rendering for player models

2. **Trigger Bot** (`trigger.rs`):
   - Automatic shooting when crosshair is on enemy
   - Configurable delays and randomization
   - State machine: Idle → Pending → Active → Sleep
   - Anti-detection features

3. **Anti-Aim Punch** (`aim.rs`):
   - Compensates for weapon recoil
   - Calculates mouse movements to counter punch angles
   - Uses sensitivity and punch angle vectors

4. **Bomb Indicator** (`bomb.rs`):
   - Shows C4 timer and defuse progress
   - Visual countdown and alerts
   - Defuser information

5. **Spectators List** (`spectators_list.rs`):
   - Shows who is currently spectating the player
   - Real-time spectator tracking

6. **Grenade Helper** (`grenade_helper.rs`):
   - Projectile trajectory prediction
   - Grenade path visualization

7. **Sniper Crosshair** (`sniper_crosshair.rs`):
   - Custom crosshair when scoped
   - Replaces default sniper scope

#### Settings System (`settings/`)

- **Configuration Management** (`config.rs`): Persistent settings storage using JSON
- **UI Interface** (`ui.rs`): ImGui-based settings interface
- **Hotkey System** (`hotkey.rs`): Key binding and toggle management
- **ESP Settings** (`esp.rs`): ESP-specific configuration options

#### View System (`view/`)

- **World Coordinate System** (`world.rs`): 3D to 2D projection calculations
- **Crosshair Management** (`crosshair.rs`): Local player crosshair state tracking
- **Key Toggle System** (`key_toggle.rs`): Toggle state management for features

#### Utilities (`utils/`)

- **ImGui Extensions** (`imgui.rs`): Text with shadow rendering, UI helper functions
- **Console I/O** (`console_io.rs`): Error handling and console detection
- **General Utilities** (`mod.rs`): URL opening, system utilities

### 2. CS2 Library (`cs2/src/`)

Core library for interfacing with the CS2 game process.

#### Memory Management (`handle.rs`)

**CS2Handle**: Main interface to CS2 process
- **Driver Interface**: Uses kernel driver for memory access (vtd_libum)
- **Process Information**: Tracks CS2 modules (client.dll, engine2.dll, etc.)
- **Memory Operations**: Read/write primitive types, slices, and strings
- **Pattern Scanning**: Find memory signatures for dynamic offset resolution

#### Schema System (`schema/`)

- **Schema Resolution**: Automatic detection of CS2 class structures and offsets
- **Runtime Adaptation**: Handles CS2 updates by resolving new offsets dynamically
- **Class Definitions**: Strongly-typed interfaces for CS2 game objects

#### State Management (`state/`)

**State Pattern Implementation**:
- **StateRegistry**: Centralized state container with caching
- **Cache Types**: Persistent, Timed, Volatile caching strategies
- **State Invalidation**: Automatic cleanup of stale data

**Key States**:
- `StateGlobals`: Global game state (tick count, etc.)
- `StateLocalPlayerController`: Local player controller entity
- `StateEntityList`: All game entities (players, items, etc.)
- `StateBuildInfo`: CS2 build information
- `StateCurrentMap`: Current map information

#### Entity System (`entity/`)

- **Entity Management** (`list.rs`): Tracking all game entities
- **Controller Entities** (`controller.rs`): Player controller objects
- **Entity Identity** (`identity.rs`): Entity ID and handle management

#### Game Data Structures

- **Offsets** (`offsets.rs`): Memory offset definitions and resolution
- **ConVars** (`convar.rs`): Console variable access
- **Models** (`model.rs`): Player model information
- **Weapons** (`weapon.rs`): Weapon data and properties

### 3. Overlay System (`overlay/src/`)

Graphics overlay system for rendering enhancements over the game.

#### Core Architecture (`lib.rs`)

**System Structure**:
```rust
pub struct System {
    pub event_loop: EventLoop<()>,
    pub overlay_window: Window,
    pub platform: WinitPlatform,
    pub imgui: Context,
    pub window_tracker: WindowTracker,
    renderer: Box<dyn RenderBackend>,
}
```

**Rendering Backends**:
- **DirectX** (`directx/mod.rs`): Windows DirectX 11 renderer
- **Vulkan** (`vulkan/mod.rs`): Cross-platform Vulkan renderer  
- **OpenGL** (`opengl/mod.rs`): OpenGL renderer

#### Window Management

- **Window Tracking** (`window_tracker.rs`): Monitors target application window
- **Overlay Positioning**: Maintains overlay position relative to game window
- **Transparency**: Creates transparent overlay window with click-through

#### Input System (`input.rs`)

- **Keyboard Input**: Key state tracking and event handling
- **Mouse Input**: Mouse position and button state
- **Input Routing**: Forwards input to ImGui or blocks from game

#### Font System (`font.rs`)

- **Multi-language Support**: Unicode text rendering
- **Font Atlas**: Efficient font texture management
- **Custom Fonts**: Roboto, Noto Sans, Unifont for wide character support

### 4. CS2 Schema System (`cs2-schema/`)

Automated system for resolving CS2 game structures and memory offsets.

#### Schema Provider (`provider/`)

**Provider Interface**:
```rust
pub trait SchemaProvider: Send + Sync {
    fn resolve_offset(&self, offset: &OffsetInfo) -> Option<u64>;
}
```

**Provider Types**:
- **Runtime Provider**: Reads CS2's schema system at runtime
- **File Provider**: Loads pre-dumped schema from JSON file
- **Cached Provider**: Caches resolved offsets for performance

#### Schema Definition (`definition/`)

- **Class Definitions** (`definition_class.rs`): Game class structure definitions
- **Enum Definitions** (`definition_enum.rs`): Game enum definitions
- **Inheritance System** (`inheritage.rs`): Class inheritance resolution

#### Code Generation (`generated/`)

- **Automatic Generation**: Generates Rust code from schema definitions
- **Type Safety**: Strongly-typed access to game structures
- **Build Integration**: Integrated into build process

#### Schema Dumper (`dumper/`)

Standalone tool for dumping CS2 schema to JSON:
```bash
schema-dumper.exe output.json [--client-only]
```

### 5. Radar System (`radar/`)

External radar functionality accessible via web browser.

#### Architecture

```
CS2 Game ←→ Radar Client ←→ Radar Server ←→ Web Client
```

#### Client (`client/`)

**CS2 Data Collection**:
- **RadarGenerator**: Extracts player positions, bomb state, etc.
- **Real-time Updates**: Continuously monitors game state
- **Data Publishing**: Sends updates to radar server

#### Server (`server/`)

**WebSocket Server**:
- **Session Management**: Handles multiple radar sessions
- **Client Coordination**: Manages publishers and subscribers
- **Real-time Broadcasting**: Distributes game state to connected clients

#### Web Client (`web/`)

**React-based Web Application**:
- **Real-time Radar**: Interactive map with player positions
- **Map Support**: Multiple CS2 maps with custom overlays
- **Responsive UI**: Works on desktop and mobile devices

**Technology Stack**:
- React + TypeScript
- WebSocket communication
- Canvas-based rendering
- Webpack build system

### 6. Utility Libraries (`utils/`)

#### State Management (`utils/state/`)

**StateRegistry System**:
```rust
pub trait State: Any + Sized + Send {
    type Parameter: Hash + PartialEq;
    fn create(states: &StateRegistry, param: Self::Parameter) -> anyhow::Result<Self>;
    fn cache_type() -> StateCacheType;
    fn update(&mut self, states: &StateRegistry) -> anyhow::Result<()>;
}
```

**Features**:
- **Type-safe State Access**: Compile-time type checking
- **Automatic Caching**: Configurable cache strategies
- **Dependency Resolution**: Automatic state dependency management
- **Memory Efficient**: Controlled memory usage with capacity limits

## Data Flow

### 1. Initialization Flow

```
1. Load Settings → 2. Create CS2 Handle → 3. Setup Schema → 4. Initialize Overlay
                                     ↓
5. Create State Registry → 6. Load Enhancements → 7. Start Main Loop
```

### 2. Main Loop Flow

```
Update Phase:
┌─────────────────┐
│  Read CS2 Data  │ → Cache in StateRegistry
└─────────────────┘
         │
         ▼
┌─────────────────┐
│Update Enhance-  │ → Process game logic
│    ments        │   (ESP, aimbot, etc.)
└─────────────────┘

Render Phase:
┌─────────────────┐
│  Render UI      │ → ImGui interface
└─────────────────┘
         │
         ▼
┌─────────────────┐
│Render Enhance-  │ → Draw overlays
│    ments        │   (ESP, crosshairs)
└─────────────────┘
         │
         ▼
┌─────────────────┐
│ Present Frame   │ → Display to screen
└─────────────────┘
```

### 3. Memory Reading Flow

```
CS2 Process ←─── Kernel Driver ←─── CS2Handle ←─── Enhancement
     │                                   │
     │                                   ▼
     └─── Pattern Scanning ──── Schema System ──── StateRegistry
```

## Security Features

### 1. Anti-Detection Measures

- **Kernel Driver Interface**: Uses signed kernel driver for memory access
- **No DLL Injection**: External process approach
- **Randomized Delays**: Anti-pattern detection in trigger bot
- **External Overlay**: Separate overlay process

### 2. Privilege Management

- **Non-Admin Execution**: Warns against running as administrator
- **Screen Capture Protection**: Optional protection from screenshots
- **Driver Validation**: Checks for known problematic drivers

## Configuration System

### Settings Structure

```rust
pub struct AppSettings {
    // ESP Settings
    pub esp_mode: KeyToggleMode,
    pub esp_players: bool,
    pub esp_distance: f32,
    pub esp_team_filter: TeamFilter,
    
    // Aimbot Settings  
    pub aim_assist_recoil: bool,
    
    // Trigger Bot Settings
    pub trigger_bot_mode: KeyToggleMode,
    pub trigger_delay_min: u32,
    pub trigger_delay_max: u32,
    
    // Keybindings
    pub key_esp_toggle: HotKey,
    pub key_trigger_bot: HotKey,
    
    // UI Settings
    pub metrics: bool,
    pub debug_window: bool,
}
```

### Persistence

- **JSON Configuration**: Human-readable settings file
- **Hot Reload**: Settings can be modified while running
- **Validation**: Input validation and sanity checks

## Build System

### Workspace Structure

```toml
[workspace]
members = [
    "controller",
    "cs2", 
    "cs2-schema/*",
    "overlay",
    "radar/*",
    "utils/*"
]
```

### Dependencies

**Core Dependencies**:
- `anyhow`: Error handling
- `tokio`: Async runtime
- `imgui`: GUI framework
- `raw-struct`: Memory structure reading
- `obfstr`: String obfuscation
- `serde`: Serialization

**Platform-specific**:
- `windows`: Windows API bindings
- `vtd_libum`: Kernel driver interface

## Performance Considerations

### 1. Memory Management

- **State Caching**: Reduces redundant memory reads
- **Batch Operations**: Groups related memory operations
- **Reference Counting**: Efficient shared data access

### 2. Rendering Optimization

- **Frame Rate Limiting**: Maintains consistent 60 FPS
- **Culling**: Only renders visible elements
- **Font Caching**: Efficient text rendering

### 3. Update Frequency

- **Differential Updates**: Only processes changed data
- **Priority System**: Critical updates processed first
- **Timeout Handling**: Prevents infinite update loops

## Error Handling

### Strategy

1. **Graceful Degradation**: Continue operation when possible
2. **User Feedback**: Clear error messages to users  
3. **Logging**: Comprehensive logging for debugging
4. **Recovery**: Automatic recovery from transient failures

### Error Types

- **Memory Access Errors**: Handle CS2 process issues
- **Schema Resolution Errors**: Handle CS2 update incompatibilities
- **Rendering Errors**: Handle graphics driver issues
- **Network Errors**: Handle radar connectivity issues

## Future Extensibility

### Plugin System Potential

The enhancement trait system provides a foundation for:
- **Dynamic Loading**: Runtime enhancement loading
- **Configuration API**: Standardized settings interface
- **Event System**: Inter-enhancement communication

### Additional Features

- **More Game Support**: Extensible to other Source 2 games
- **Advanced AI**: Machine learning-based enhancements
- **Network Features**: Multiplayer coordination
- **Mobile Integration**: Mobile companion apps

## Development Guidelines

### Code Style

- **Rust Best Practices**: Follow Rust conventions
- **Error Handling**: Use `anyhow::Result` for error propagation
- **Documentation**: Comprehensive inline documentation
- **Testing**: Unit tests for critical functionality

### Security Guidelines

- **Obfuscation**: Use `obfstr!` for sensitive strings
- **Validation**: Validate all external inputs
- **Minimal Privileges**: Request minimum required permissions
- **Audit Trail**: Log security-relevant operations

This documentation provides a comprehensive overview of the Valthrun CS2 enhancer architecture, serving as a reference for understanding the codebase structure, data flow, and implementation details. 