# OpenVR Mumble Link

Syncs mumble's positional audio system to your OpenVR HMD's position, relative to your playspace.
When chatting with others in VR who are, their voice should then sound as if you're together in the same
room (rather than just talking in your head).

Unfortunately, mumble's spatialization isn't that good (just basic panning and inter-ear delay). I plan
to try to integrate SteamAudio's HRTF-based spatialization into mumble to make this better.

## Usage

1. Run SteamVR.
2. Setup mumble to use positional audio with the "link" plugin. The [Minecraft mumble-link README has instructions with pictures](https://github.com/zsawyer/MumbleLink?tab=readme-ov-file#installing-the-mod)
3. Run `ovr_mumble_link.exe`. It should run in the background; there's no GUI.

### How do I know it's working?

In the default mumble client's menu, there's a **Developer** menu with a **Positional Audio Viewer**
item in it. If the link is working, you should see the headset position and rotation in there, and
"OpenVR" somewhere in the context.