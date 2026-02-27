# Plan: Projection Overlay Prototype Spike

## Context

The ovr_mumble_link project needs to render 3D objects (remote user
representations) in SteamVR using projection overlays
(`SetOverlayTransformProjection`). This API is poorly documented and has no
published working examples. Before building the full system, we need a
standalone prototype to validate the matrix/frustum setup and prove the
rendering pipeline works.

The prototype is also a feasibility gate: if projection overlays can't be made
to work, we fall back to positioned quad overlays (simpler but no stereo depth).

## Key Research Findings

- **ovr_overlay** crate does NOT wrap `SetOverlayTransformProjection`. We must
  call through raw sys bindings or add a thin wrapper.
- **vulkano** (not wgpu) is the right GPU library. WayVR proves the entire
  vulkano -> VRVulkanTextureData_t -> SetOverlayTexture pipeline. wgpu's
  abstraction makes native handle extraction painful with no existing examples.
- Stereo requires **two overlay handles** (one per eye), each with its own
  rendered texture and projection frustum.
- `GetProjectionRaw()` returns tangent values in the same format as
  `VROverlayProjection_t`, so the frustum data maps directly.
- `GetEyeToHeadTransform()` gives the per-eye offset needed to compute each
  eye's world position.

## What We're Building

A Rust binary crate (`crates/projection-spike/`) in the workspace that:

1. Initializes OpenVR as an overlay app
2. Initializes Vulkan via vulkano (following WayVR's pattern)
3. Creates two overlay handles (left eye, right eye)
4. Each frame:
   a. Gets HMD pose, eye transforms, and projection frustums from OpenVR
   b. Renders a simple 3D scene (colored cubes at fixed world positions) from
      each eye's perspective using vulkano
   c. Submits textures to overlays via `set_image_vulkan()`
   d. Calls `SetOverlayTransformProjection()` with the eye pose + frustum
5. Listens for keyboard/controller input to cycle between transform variants

### Transform Variants to Test

The main unknown is how the `pmatTrackingOriginToOverlayTransform` matrix
relates to the rendering camera. Variants to test via keyboard toggle:

1. **Identity eye pose**: Pass eye world pose directly as the overlay transform,
   use GetProjectionRaw tangents as the frustum
2. **Inverted eye pose**: Pass the inverse of the eye world pose
3. **Head pose (not eye)**: Use head pose for both overlays, only vary frustum
4. **Origin at scene center**: Transform relative to the scene rather than the
   eye
5. **Flipped Z**: Try negating Z in the transform (OpenVR coordinate
   conventions)
6. **Swapped eyes**: Left frustum on right overlay and vice versa

Each variant is a different interpretation of what the API expects. The user
can cycle through them in VR and immediately see which produces correct results.

### The 3D Scene

Fixed geometry, no need for dynamic content:
- 3 colored cubes (red, green, blue) at positions (0,1.5,-2), (1,1,-3),
  (-1,2,-2.5) in standing space
- Simple flat-shaded rendering (no lighting needed)
- A ground plane grid for spatial reference
- All positioned in absolute tracking space so they appear at fixed world
  locations

### Keyboard Controls

- **1-6**: Switch between transform variants
- **R**: Reset overlays (destroy + recreate)
- **Q**: Quit
- **+/-**: Adjust render resolution

Display which variant is active via console output (and optionally a small
dashboard overlay showing current mode).

## Implementation Steps

### Step 1: Project setup
- Convert ovr_mumble_link to a Cargo workspace
- Add `crates/projection-spike/` as a binary crate in the workspace
- Workspace-level dependencies: `ovr_overlay` (git dep on galister's fork),
  `vulkano`, `vulkano-shaders`, `glam`
- Move existing src/main.rs code into `crates/link-app/` (or leave at root)
- Verify ovr_overlay builds against the galister fork

### Step 2: OpenVR + Vulkan initialization
- Init OpenVR as `VRApplication_Overlay`
- Init Vulkan via vulkano with OpenVR-required extensions (follow WayVR's
  `init_openvr_graphics` pattern)
- Create two render target images (one per eye) with
  `ImageUsage::TRANSFER_SRC | COLOR_ATTACHMENT | SAMPLED`
- Create two overlay handles

Reference: WayVR's `init_openvr_graphics` in `wayvr/src/graphics/mod.rs`

### Step 3: Rendering pipeline
- Write a minimal vulkano vertex+fragment shader (flat-shaded colored
  triangles)
- Hard-code cube vertex data (36 vertices for 12 triangles, or indexed)
- Hard-code ground plane grid vertex data
- Create a render pass targeting the eye textures
- Uniform buffer for view-projection matrix (updated per eye per frame)

### Step 4: Main loop
- Poll OpenVR events
- Get HMD pose via `GetDeviceToAbsoluteTrackingPose(Standing, 0.0)`
- Get per-eye transforms: `GetEyeToHeadTransform(Eye_Left/Right)`
- Get per-eye frustums: `GetProjectionRaw(Eye_Left/Right)`
- Compute view matrices for each eye
- Render scene to each eye texture
- Submit textures via `set_image_vulkan()`
- Call `SetOverlayTransformProjection()` with current variant's matrix setup
- Poll keyboard for variant switching

### Step 5: SetOverlayTransformProjection FFI
- The ovr_overlay crate doesn't wrap this. Options:
  a. Add a method to ovr_overlay's OverlayManager (fork or PR)
  b. Call the raw C function through ovr_overlay_sys
  c. Use openvr-sys2 crate which has raw bindings
- Start with option (b) since it's the smallest change. We already have the
  overlay handle; just need the raw IVROverlay function pointer.

### Step 6: Testing and iteration
- Run with SteamVR, look at the cubes
- Cycle through variants to find which produces correct stereo 3D
- Once correct variant is found, document the matrix conventions
- Test: cubes should appear at fixed world positions with correct stereo
  depth, and moving your head should produce correct parallax

## Key Reference Code

- **WayVR Vulkan init**: `wayvr/src/graphics/mod.rs` (~line 398,
  `init_openvr_graphics`) - how to create Vulkan instance/device with
  OpenVR-required extensions
- **WayVR overlay texture upload**: `wayvr/src/backend/openvr/overlay.rs`
  (~lines 269-322) - VRVulkanTextureData_t population and set_image_vulkan()
- **WayVR overlay creation**: `wayvr/src/backend/openvr/overlay.rs`
  (~lines 36-44) - CreateOverlay pattern
- **OpenVR projection API**: `openvr/headers/openvr.h` (~line 4337,
  `SetOverlayTransformProjection`; ~line 4170, `VROverlayProjection_t`)
- **OpenVR eye transforms**: `openvr/headers/openvr.h` (~line 2303,
  `GetProjectionRaw`; `GetEyeToHeadTransform`)
- **Existing project**: `src/main.rs` - current HMD pose polling and
  coordinate conversion

## Verification

1. App compiles and runs on Windows with SteamVR
2. Two overlays are created (visible in SteamVR overlay debug)
3. At least one transform variant produces visually correct results:
   - Cubes appear at fixed positions in the room
   - Moving head produces correct parallax
   - Closing one eye shows each eye sees a slightly different perspective
   - No obvious distortion or wrong-eye assignment
4. Document which variant works in a comment/README for the main project

## Output

The prototype produces:
- Confirmed working matrix convention for projection overlays
- A minimal but complete vulkano + OpenVR overlay rendering pipeline
- Code that can be extracted into the main project's overlay-app crate

## Mumble Plugin Research Summary

While not part of this spike, parallel research found:
- **mumble-sys** Rust crate exists but is stale (nightly-only, 2021, zero users)
- **FGCom-mumble** ([hbeni/fgcom-mumble](https://github.com/hbeni/fgcom-mumble))
  is the only substantial sendData() user in the wild
- **Official C template** ([mumble-voip/mumble-plugin-template](https://github.com/mumble-voip/mumble-plugin-template))
  and **C++ wrapper** ([mumble-voip/mumble-plugin-cpp](https://github.com/mumble-voip/mumble-plugin-cpp))
  exist and are the best starting points
- For our Rust plugin: likely write our own thin FFI rather than depend on
  mumble-sys
- Official docs in mumble repo at `docs/dev/plugins/` are good
