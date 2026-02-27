# OpenVR Mumble Link - Design Document

## Vision

A social VR presence system built on Mumble and SteamVR. Users in the same
Mumble channel appear as visible entities in each other's VR playspace with
spatial audio, creating a "shared warehouse" effect where everyone is in the
same big room playing their own games but can see and talk to each other.

## Current State

The existing `ovr_mumble_link` is a ~100-line Rust app that:
- Connects to OpenVR as a background app
- Polls HMD pose at 50Hz (TrackingUniverseStanding)
- Writes position + orientation to Mumble's shared memory (MumbleLink)
- Handles coordinate conversion (OpenVR right-handed -> Mumble left-handed)
- Per-player offsets arise naturally from each user's playspace origin
- Uses `mumble-link` crate + `ovr_overlay`/`ovr_overlay_sys` (galister's fork)

This provides spatial audio positioning but is **write-only** - you cannot see
where other users are.

## Decisions

- **Architecture**: Two components (Mumble plugin + standalone overlay app) with
  shared memory IPC
- **Rendering approach**: Projection overlays (`SetOverlayTransformProjection`)
  for stereoscopic 3D rendering of remote users, as a feasibility investment
  toward future avatar rendering
- **Platform**: Windows first, Linux second
- **Language**: Rust for both components
- **Server**: Private Mumble server available (can tune rate limits)

---

## Architecture

### Component 1: Mumble Plugin (shared library, loaded by Mumble)

A native Mumble plugin (.dll/.so) implementing the v2 Plugin API
(`MumblePlugin.h`). Written in Rust with `#[no_mangle] extern "C"` exports.

Responsibilities:
- **Positional audio**: Implements `mumble_fetchPositionalData()` to feed HMD
  position/orientation into Mumble's spatial audio system
- **Pose broadcast**: Uses `sendData()` to transmit local user's pose to other
  users in the channel
- **Pose reception**: Implements `mumble_onReceiveData()` to receive remote
  users' pose data
- **IPC**: Writes/reads shared memory region to communicate with overlay app

Required exported functions (~10, all simple C types):
```
mumble_init, mumble_shutdown, mumble_getName, mumble_getVersion,
mumble_getAPIVersion, mumble_registerAPIFunctions, mumble_releaseResource,
mumble_getFeatures, mumble_initPositionalData, mumble_fetchPositionalData,
mumble_shutdownPositionalData, mumble_onReceiveData
```

### Component 2: OpenVR Overlay Application (standalone Rust executable)

- Connects to OpenVR as `VRApplication_Overlay`
- Reads HMD + controller poses at 50Hz
- Writes local pose to shared memory (for Mumble plugin to read)
- Reads remote user poses from shared memory (written by Mumble plugin)
- Renders remote users as 3D objects via projection overlays
- Renders a 2D dashboard overlay for settings/status

### IPC: Shared Memory

A named shared memory region (`OvrMumbleLink` on Windows,
`/OvrMumbleLink.<uid>` on Linux) with a fixed-size structure:

```
┌─────────────────────────────────────────────────┐
│ Header                                          │
│   version: u32                                  │
│   local_sequence: AtomicU64  (overlay writes)   │
│   remote_sequence: AtomicU64 (plugin writes)    │
│   num_remote_users: AtomicU32                   │
├─────────────────────────────────────────────────┤
│ Local Pose (written by overlay app)             │
│   position: [f32; 3]                            │
│   orientation: [f32; 4]  (quaternion)           │
│   left_hand_pos: [f32; 3]                       │
│   left_hand_rot: [f32; 4]                       │
│   right_hand_pos: [f32; 3]                      │
│   right_hand_rot: [f32; 4]                      │
├─────────────────────────────────────────────────┤
│ Remote Users[MAX_USERS] (written by plugin)     │
│   For each:                                     │
│     user_id: u32                                │
│     username: [u8; 64]                          │
│     position: [f32; 3]                          │
│     orientation: [f32; 4]                       │
│     left_hand_pos: [f32; 3]                     │
│     left_hand_rot: [f32; 4]                     │
│     right_hand_pos: [f32; 3]                    │
│     right_hand_rot: [f32; 4]                    │
│     last_update_ms: u64                         │
│     offset_index: u32                           │
└─────────────────────────────────────────────────┘
```

Double-buffered or atomic-sequence-guarded to avoid tearing. MAX_USERS = 16 is
plenty.

```
┌─────────────────┐     shared memory      ┌──────────────────┐
│  OpenVR Overlay  │◄─────────────────────►│  Mumble Plugin    │
│  App (Rust)      │  local pose out        │  (.dll/.so)       │
│                  │  remote poses in       │                   │
│  - HMD tracking  │  channel state in      │  - positional     │
│  - 3D rendering  │                        │    audio          │
│  - projection    │                        │  - sendData()     │
│    overlays      │                        │  - onReceiveData()│
│  - settings UI   │                        │                   │
└─────────────────┘                        └──────────────────┘
         │                                          │
         ▼                                          ▼
    SteamVR/OpenVR                           Mumble Server
    (compositor)                         (relays voice + data)
```

---

## Mumble Plugin API: Key Constraints

### Positional Audio (built-in, reliable)
- Plugin provides: position[3], front[3], top[3] for avatar and camera
- Context string determines who hears each other (all users set same context)
- Transmitted alongside voice data, no rate limit
- **Key limitation**: Remote user positions are consumed internally by Mumble
  for audio spatialization. The plugin API does NOT expose remote user positions
  back to plugins. We must use `sendData()` for visual positioning.

### Plugin Data Channel (`sendData()`)
- Arbitrary binary up to **1024 bytes** per message
- `dataID` string (up to 100 bytes) for message type routing
- **Rate-limited by server** - excess messages silently dropped
- Default recommendation: ~1 second intervals
- With a private server, rate limit is likely configurable (need to verify)
- All active plugins on receiving client see the data

### Packet Budget (1024 bytes)

| Data                          | Size     |
|-------------------------------|----------|
| Header (version, flags, seq)  | 8 bytes  |
| Head position (vec3)          | 12 bytes |
| Head orientation (quat)       | 16 bytes |
| Left hand pos + rot           | 28 bytes |
| Right hand pos + rot          | 28 bytes |
| Linear velocity (vec3)        | 12 bytes |
| Angular velocity (vec3)       | 12 bytes |
| Timestamp (ms)                | 8 bytes  |
| **Total basic pose**          | **124 bytes** |
| Remaining for future IK       | ~900 bytes |

Full humanoid IK (20 bones x 28 bytes) = 560 bytes, fits easily.

### Dealing with Update Rate

At ~1Hz, remote positions jump. Mitigations:
1. **Extrapolation**: Include linear + angular velocity in packets. Receiver
   extrapolates between updates. Good enough for slow movement.
2. **Server tuning**: On our private server, push rate to 5-10Hz if possible.
3. **Hermite interpolation**: Use position + velocity at both endpoints for
   smooth curves between updates.
4. **Future**: Direct UDP/WebRTC data channel if mumble proves too limiting.

---

## Rendering: Projection Overlays

### How Projection Overlays Work

`SetOverlayTransformProjection()` tells SteamVR: "I rendered a 3D scene from
this viewpoint with this frustum for this eye. Please display it correctly."

```cpp
EVROverlayError SetOverlayTransformProjection(
    VROverlayHandle_t ulOverlayHandle,
    ETrackingUniverseOrigin eTrackingOrigin,
    const HmdMatrix34_t* pmatTrackingOriginToOverlayTransform,  // camera pose
    const VROverlayProjection_t* pProjection,                   // frustum
    EVREye eEye                                                 // which eye
);
```

- `pmatTrackingOriginToOverlayTransform`: The world-space pose of the virtual
  camera that rendered this texture (i.e., the eye position/orientation)
- `pProjection`: Frustum tangent values (fLeft, fRight, fTop, fBottom) matching
  the projection used to render the texture
- `eEye`: Which eye this overlay is for (Eye_Left or Eye_Right)

### Stereo Pipeline

Two overlay handles are needed (one per eye). Each frame:

1. Get eye transforms from OpenVR:
   - `GetEyeToHeadTransform(Eye_Left)` and `GetEyeToHeadTransform(Eye_Right)`
   - `GetProjectionRaw(Eye_Left, ...)` and `GetProjectionRaw(Eye_Right, ...)`
   - These return the same tangent format as `VROverlayProjection_t`
2. Compute eye world positions: `head_pose * eye_to_head_transform`
3. Render 3D scene from each eye's perspective to a texture
4. Submit textures: `SetOverlayTexture(left_handle, left_texture)` etc.
5. Set projection: `SetOverlayTransformProjection(left_handle, Standing,
   &left_eye_pose, &left_projection, Eye_Left)`

### Rendering Pipeline (vulkano)

Use `vulkano` for GPU-accelerated rendering to texture:
- Create two render targets (one per eye), resolution matching or near HMD
  resolution
- Simple forward renderer: colored cubes for MVP, models later
- No post-processing needed (SteamVR handles reprojection)
- Render at overlay app's frame rate (~50Hz), independent of SteamVR compositor

**Why vulkano over wgpu**: WayVR proves the entire vulkano -> OpenVR texture
pipeline. Vulkan handle extraction is trivial (`.handle().as_raw()`). wgpu's
abstraction layer makes native handle extraction painful (unstable `as_hal`
API, image layout management issues) with zero existing OpenVR examples. Vulkan
works on both Linux and Windows (SteamVR always installs the Vulkan runtime).

### What We Render (MVP)

For each remote user:
- A colored wireframe or solid cube (~0.3m) at their head position
- Oriented by their head quaternion (if available) or axis-aligned
- A floating name label above the cube (textured quad, part of the 3D scene)

This is deliberately simple geometry to validate the projection overlay pipeline
before investing in model loading.

### Fallback: Quad Overlays

If projection overlays prove too difficult to get working correctly (the matrix
setup is notoriously finicky and poorly documented), fall back to:
- One `SetOverlayTransformAbsolute()` quad per user with a CPU-rendered texture
- No stereo depth but still shows user presence and position
- Much simpler, known-working approach

---

## Shared Playspace Model

Each user's HMD position is relative to their standing playspace origin.
Per-player offsets place users ~3m apart:

```
User 0: offset = (0, 0, 0)
User 1: offset = (3, 0, 0)
User 2: offset = (6, 0, 0)
...
```

Offset assignment: based on sorted Mumble user ID within the channel. When a
user joins/leaves, offsets may shift - this is acceptable for MVP. Later could
use stable assignment (e.g., hash-based or explicit selection).

The local user applies their own offset before writing to shared memory. Remote
users' positions arrive already offset.

---

## Build & Dependencies

### Mumble Plugin (Rust cdylib)

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
# Shared memory IPC
# Math (glam)
# Serialization for packets (bincode or manual)
```

Produces `ovr_mumble_plugin.dll` / `libovr_mumble_plugin.so`.
Mumble discovers plugins in its `plugins/` directory.

### Overlay App (Rust binary)

```toml
[dependencies]
ovr_overlay = { git = "..." }    # OpenVR overlay management
vulkano = "..."                  # Vulkan GPU rendering
glam = "..."                     # Math
# Shared memory IPC (shared with plugin crate)
```

### Workspace Structure

```
ovr_mumble_link/
├── Cargo.toml                  # workspace
├── crates/
│   ├── mumble-plugin/          # cdylib - Mumble plugin
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── overlay-app/            # binary - OpenVR overlay + rendering
│   │   ├── Cargo.toml
│   │   └── src/main.rs
│   └── shared/                 # shared types, IPC, packet format
│       ├── Cargo.toml
│       └── src/lib.rs
├── design.md
└── README.md
```

---

## MVP Scope

### What "done" looks like

Two VR users on Windows, each running SteamVR + Mumble with the plugin +
overlay app. They are in the same Mumble channel. Each user sees colored cubes
representing the other users floating in their VR space at the appropriate
offset positions, with spatial audio from Mumble making voices come from the
cube directions.

### MVP tasks

1. **Shared crate**: IPC shared memory types, packet serialization, coordinate
   transforms
2. **Mumble plugin**: Positional audio + sendData/receiveData + shared memory
   writes
3. **Overlay app - tracking**: HMD pose polling, shared memory writes, remote
   pose reading
4. **Overlay app - projection overlays**: Create stereo projection overlays,
   vulkano render pipeline, render cubes at remote user positions
5. **Overlay app - dashboard**: Simple 2D status overlay showing connected
   users
6. **Integration testing**: Two users, verify spatial audio + visual presence

---

## Future Milestones

### M2: Orientation & Hands
- Head orientation displayed (oriented cubes)
- Hand positions displayed (small spheres or cubes at hand locations)
- Evaluate actual `sendData()` throughput on the server

### M3: Smooth Motion
- Velocity-based extrapolation between updates
- Hermite interpolation for smooth curves
- Server rate limit tuning if needed

### M4: 3D Avatars
- Load VRM/glTF models
- Render skinned meshes in vulkano
- Transmit IK bone data via sendData (fits in 1KB)

### M5: Alternative Backend (if needed)
- LiveKit WebRTC: audio + unrestricted data channel
- Eliminates rate limits for pose/IK data
- Enables higher-fidelity streaming

---

## Open Questions

1. **sendData() actual rate limits**: Need to test empirically. The Mumble test
   plugin explicitly tests hitting the rate limiter. Check if murmur (server)
   has a config knob for this. If we can get 5-10Hz, motion will be acceptably
   smooth with interpolation.

2. **Projection overlay matrix setup**: The API is poorly documented and no
   reference codebase uses it. Per Valve: "call it once per eye per frame."
   The frustum tangent values from `GetProjectionRaw()` should map directly to
   `VROverlayProjection_t`. Main risk is getting the coordinate transforms
   right. Plan to prototype this early as a go/no-go gate.

3. **Plugin discovery**: Verify Mumble loads Rust-built .dll/.so correctly.
   May need specific naming or a manifest. The plugin directory and naming
   convention need testing.

4. **ovr_overlay crate**: Current project uses a local path dependency
   (`../ovr_overlay`). WayVR uses galister's fork via git. Need to use the git
   dependency or vendor it.

5. **vulkano texture sharing with OpenVR**: WayVR proves this works. The
   `VRVulkanTextureData_t` struct needs raw Vulkan handles which vulkano
   exposes trivially via `.handle().as_raw()`. Vulkan runtime is always
   available on both Linux and Windows when SteamVR is installed.

---

## References

### Mumble
- Mumble v2 Plugin API header: `mumble/plugins/MumblePlugin.h`
- Mumble plugin docs: `mumble/docs/dev/plugins/` (CreatePlugin.md, MumbleAPI.md, PluginAPI.md)
- Test plugin (sendData example): `mumble/plugins/testPlugin/testPlugin.cpp`
- GTA V plugin (positional audio example): `mumble/plugins/gtav/gtav.cpp`
- Link plugin (shared memory): `mumble/plugins/link/link.cpp`
- [Official plugin template (C)](https://github.com/mumble-voip/mumble-plugin-template)
- [Official C++ wrapper](https://github.com/mumble-voip/mumble-plugin-cpp)
- [FGCom-mumble](https://github.com/hbeni/fgcom-mumble) - best real-world sendData() example
- [mumble-sys Rust crate](https://github.com/Dessix/rust-mumble-sys) - stale but referenced in Mumble docs

### OpenVR
- OpenVR overlay API: `openvr/headers/openvr.h` (IVROverlay ~line 4179)
- Projection overlay: `SetOverlayTransformProjection` (openvr.h ~line 4337)
- Projection struct: `VROverlayProjection_t` (openvr.h ~line 4170)
- Eye transforms: `GetProjectionRaw`, `GetEyeToHeadTransform`
- [Valve on projection overlays](https://github.com/ValveSoftware/openvr/issues/1527)
- [SteamVR overlay stereo tutorial](https://dev.to/kurohuku/appendix-3e8f)

### Rust VR Ecosystem
- [ovr_overlay](https://github.com/TheButlah/ovr_overlay) (galister's fork: ovr_overlay_oyasumi)
- WayVR overlay code: `wayvr/src/backend/openvr/overlay.rs` (~lines 269-322, texture upload)
- WayVR Vulkan init: `wayvr/src/graphics/mod.rs` (~line 398, `init_openvr_graphics`)
- DesktopPlus overlay code: `DesktopPlus/src/DesktopPlus/Overlays.cpp`
