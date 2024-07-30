use mumble_link::Position;
use mumble_link::SharedLink;
use ovr_overlay::Context;
use ovr_overlay_sys as sys;
use ovr_overlay_sys::ETrackingUniverseOrigin;
use ovr_overlay_sys::EVRApplicationType;
use rand;

fn main() {
    let context = Context::init(EVRApplicationType::VRApplication_Background)
        .expect("Failed to initialize OpenVR");

    let mut link = SharedLink::new("OpenVR",
     "openVR HMD position, relative to playspace.");
    link.set_context(b"openvr");

    // unclear if you even need an identity set; set to random in case it's required.
    // Set identity to a random string
    let random_identity = format!("user_{}", rand::random::<u64>());
    link.set_identity(&random_identity);

    let mut system = context.system_mngr();
    let interval = std::time::Duration::from_millis(20); // 50 times per second

    println!("OpenVR initialized and mumble link created, transmitting pose...");

    loop {
        std::thread::sleep(interval);

        let poses = system.get_device_to_absolute_tracking_pose(
            ETrackingUniverseOrigin::TrackingUniverseStanding,
            0.0,
        );
        // TrackedDevicePose_t { pub mDeviceToAbsoluteTracking : root :: vr :: HmdMatrix34_t , pub vVelocity : root :: vr :: HmdVector3_t , pub vAngularVelocity : root :: vr :: HmdVector3_t , pub eTrackingResult : root :: vr :: ETrackingResult , pub bPoseIsValid : bool , pub bDeviceIsConnected : bool , }
        let pose = poses.first().unwrap();

        if pose.eTrackingResult != sys::ETrackingResult::TrackingResult_Running_OK {
            println!(
                "Tracking result: {}",
                tracking_result_to_string(&pose.eTrackingResult)
            );
            continue;
        }

        // right-handed system
        // +y is up
        // +x is to the right
        // -z is forward
        // Distance unit is  meters
        let hmd_pose_right_handed = &pose.mDeviceToAbsoluteTracking.m;

        // Convert to left-handed system
        // +y is up
        // +x is to the right
        // +z is forward
        let position = [
            hmd_pose_right_handed[0][3],
            hmd_pose_right_handed[1][3],
            -hmd_pose_right_handed[2][3],
        ];

        // front is +Z vector transformd by matrix
        let front = [
            -hmd_pose_right_handed[0][2],
            -hmd_pose_right_handed[1][2],
            hmd_pose_right_handed[2][2],
        ];

        let top = [
            hmd_pose_right_handed[0][1],
            hmd_pose_right_handed[1][1],
            -hmd_pose_right_handed[2][1],
        ];

        // pub struct Position {
        //     /// The character's position in space.
        //     pub position: [f32; 3],
        //     /// A unit vector pointing out of the character's eyes.
        //     pub front: [f32; 3],
        //     /// A unit vector pointing out of the top of the character's head.
        //     pub top: [f32; 3],
        // }
        let pose = Position {
            position,
            front,
            top,
        };

        link.update(pose, pose);
    }
}

fn tracking_result_to_string(result: &sys::ETrackingResult) -> &'static str {
    match result {
        sys::ETrackingResult::TrackingResult_Uninitialized => "Uninitialized",
        sys::ETrackingResult::TrackingResult_Calibrating_InProgress => "Calibrating_InProgress",
        sys::ETrackingResult::TrackingResult_Calibrating_OutOfRange => "Calibrating_OutOfRange",
        sys::ETrackingResult::TrackingResult_Running_OK => "Running_OK",
        sys::ETrackingResult::TrackingResult_Running_OutOfRange => "Running_OutOfRange",
        _ => "Unknown tracking result",
    }
}
