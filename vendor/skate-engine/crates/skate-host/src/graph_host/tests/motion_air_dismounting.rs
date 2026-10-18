use super::*;
use crate::{
    graph_host::motion::MotionHost,
    graph_runtime::{CompiledGraph, LoadedGraph},
};
use skate_core::{
    animation::{output::attributes::AttributePayload, playback::PlaybackContext},
    graph::controller::{Frame, Host},
};
use skate_data::{
    collections::Collections,
    state_graph::{StateGraph, binding::Binding},
};

#[test]
#[ignore = "requires private stock graphs and animation banks"]
fn stock_air_dismount_revert_is_published_only_for_the_clip_marker() {
    let root = std::path::PathBuf::from(
        std::env::var_os("SKATE3_ASSET_ROOT").expect("Set SKATE3_ASSET_ROOT"),
    );
    let source =
        StateGraph::load(&root.join("private/stock/data/state/MotionGraph_OnBoard.stategraph"))
            .unwrap();
    let binding = Binding::from_graph(&source).unwrap();
    let runtime = CompiledGraph::from_binding(&binding).unwrap();
    let graph = LoadedGraph {
        source,
        binding,
        runtime,
    };
    let banks = skate_data::animation_banks::AnimationBanks::load(&root).unwrap();
    let mut host = MotionHost::from_graph(
        &graph,
        &Collections::load(&root).unwrap(),
        banks.metadata().unwrap(),
        PlaybackContext {
            is_switch: Some(false),
            is_mirrored: Some(false),
            board_available: Some(true),
            pro_skater: encode(b""),
            transition_override: None,
        },
    )
    .unwrap();
    host.animation.current = Some(PlaybackTree::Clip {
        name: "dismount-test".into(),
        clip: PlaybackClip::new(61.0, 60.0, 1.0, 0, Vec::new()),
    });
    host.animation.skater_animation_flags = Some(0x0802_0000);

    let frame = Frame {
        dt: 1.0 / 120.0,
        current: None,
        last: None,
        state_times: Vec::new(),
    };
    const BEHAVIOUR: usize = 3207;
    host.allocate(BEHAVIOUR, &frame);
    host.begin(BEHAVIOUR, [0; 6], &frame);
    host.update(BEHAVIOUR, [0; 6], &frame);
    assert!(host.errors.is_empty(), "{:?}", host.errors);
    assert_eq!(host.animation.air_dismount_revert_frames, None);
    assert_eq!(host.animation.skater_animation_flags, Some(0x0802_0000));

    host.animation.tree_attributes.push(AnimationAttribute {
        name: encode(b"airdismountrevert"),
        payload: AttributePayload([Some(0); 6]),
        begin_time: 0.0,
        end_time: 0.0,
        status: 0,
        kind: 0,
        sequence_id: -1,
    });
    host.animation.seek_current_fraction(0.5);
    host.update(BEHAVIOUR, [0; 6], &frame);
    assert_eq!(host.animation.air_dismount_revert_frames.take(), Some(59));
    assert_eq!(host.animation.skater_animation_flags, Some(0x0812_0000));

    // The actor consumes the pending frame count and clears request bit20 at
    // animation publication, then a re-entered behavior captures the shorter
    // remaining clip duration.
    host.animation.skater_animation_flags = Some(0x0802_0000);
    host.end(BEHAVIOUR, [0; 6], &frame);
    assert_eq!(host.animation.air_dismount_revert_frames, None);
    host.begin(BEHAVIOUR, [0; 6], &frame);
    host.update(BEHAVIOUR, [0; 6], &frame);
    assert_eq!(host.animation.air_dismount_revert_frames.take(), Some(29));

    host.animation.tree_attributes.clear();
    host.animation.skater_animation_flags = Some(0x0802_0000);
    host.update(BEHAVIOUR, [0; 6], &frame);
    assert_eq!(host.animation.air_dismount_revert_frames, None);
    assert_eq!(host.animation.skater_animation_flags, Some(0x0802_0000));
    assert!(host.errors.is_empty(), "{:?}", host.errors);
}
