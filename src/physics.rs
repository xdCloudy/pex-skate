//! Versioned client-input protocol. Gameplay simulation is owned by the
//! vendored Skate 3 host, not recreated in this PEX adapter.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    pub forward: f32,
    pub steer: f32,
    pub right_x: f32,
    pub right_y: f32,
    pub triggers: [u8; 2],
    pub dpad: u8,
    pub push: bool,
    pub push_left: bool,
    pub brake: bool,
    pub dismount: bool,
    pub session_modifier: bool,
    pub respawn: bool,
    pub shove: bool,
}

impl Controls {
    /// Decode legacy controls through v7's marker and checkpoint actions.
    pub fn decode(packet: &[u8]) -> Option<Self> {
        if packet.first() == Some(&9) {
            let [
                9,
                flags,
                left_trigger,
                right_trigger,
                forward,
                steer,
                right_x,
                right_y,
                modifiers,
                _,
                _,
                _,
                _,
            ] = packet
            else {
                return None;
            };
            return Self::decode(&[
                8,
                *flags,
                *left_trigger,
                *right_trigger,
                *forward,
                *steer,
                *right_x,
                *right_y,
                *modifiers,
            ]);
        }
        match packet {
            [1, flags, forward, steer] if flags & !0x1f == 0 => Some(Self {
                forward: f32::from(*forward as i8) / 127.0,
                steer: f32::from(*steer as i8) / 127.0,
                right_x: if flags & 16 != 0 { 1.0 } else { 0.0 },
                right_y: if flags & 8 != 0 {
                    -1.0
                } else if flags & 4 != 0 {
                    1.0
                } else {
                    0.0
                },
                triggers: [0; 2],
                dpad: 0,
                push: flags & 1 != 0,
                push_left: false,
                brake: flags & 2 != 0,
                dismount: false,
                session_modifier: false,
                respawn: false,
                shove: false,
            }),
            [2, flags, forward, steer, right_x, right_y] if flags & !0x07 == 0 => Some(Self {
                forward: f32::from(*forward as i8) / 127.0,
                steer: f32::from(*steer as i8) / 127.0,
                right_x: f32::from(*right_x as i8) / 127.0,
                right_y: f32::from(*right_y as i8) / 127.0,
                triggers: [0; 2],
                dpad: 0,
                push: flags & 1 != 0,
                push_left: flags & 2 != 0,
                brake: flags & 4 != 0,
                dismount: false,
                session_modifier: false,
                respawn: false,
                shove: false,
            }),
            [
                3,
                flags,
                left_trigger,
                right_trigger,
                forward,
                steer,
                right_x,
                right_y,
            ] if flags & !0x07 == 0 => Some(Self {
                forward: f32::from(*forward as i8) / 127.0,
                steer: f32::from(*steer as i8) / 127.0,
                right_x: f32::from(*right_x as i8) / 127.0,
                right_y: f32::from(*right_y as i8) / 127.0,
                triggers: [*left_trigger, *right_trigger],
                dpad: 0,
                push: flags & 1 != 0,
                push_left: flags & 2 != 0,
                brake: flags & 4 != 0,
                dismount: false,
                session_modifier: false,
                respawn: false,
                shove: false,
            }),
            [
                4,
                flags,
                left_trigger,
                right_trigger,
                forward,
                steer,
                right_x,
                right_y,
            ] if flags & !0x7f == 0 => Some(Self {
                forward: f32::from(*forward as i8) / 127.0,
                steer: f32::from(*steer as i8) / 127.0,
                right_x: f32::from(*right_x as i8) / 127.0,
                right_y: f32::from(*right_y as i8) / 127.0,
                triggers: [*left_trigger, *right_trigger],
                dpad: (flags >> 3) & 0x0f,
                push: flags & 1 != 0,
                push_left: flags & 2 != 0,
                brake: flags & 4 != 0,
                dismount: false,
                session_modifier: false,
                respawn: false,
                shove: false,
            }),
            [
                5,
                flags,
                left_trigger,
                right_trigger,
                forward,
                steer,
                right_x,
                right_y,
            ] => Some(Self {
                forward: f32::from(*forward as i8) / 127.0,
                steer: f32::from(*steer as i8) / 127.0,
                right_x: f32::from(*right_x as i8) / 127.0,
                right_y: f32::from(*right_y as i8) / 127.0,
                triggers: [*left_trigger, *right_trigger],
                dpad: (flags >> 3) & 0x0f,
                push: flags & 1 != 0,
                push_left: flags & 2 != 0,
                brake: flags & 4 != 0,
                dismount: flags & 0x80 != 0,
                session_modifier: false,
                respawn: false,
                shove: false,
            }),
            [
                6,
                flags,
                left_trigger,
                right_trigger,
                forward,
                steer,
                right_x,
                right_y,
                modifiers,
            ] if modifiers & !1 == 0 => Some(Self {
                forward: f32::from(*forward as i8) / 127.0,
                steer: f32::from(*steer as i8) / 127.0,
                right_x: f32::from(*right_x as i8) / 127.0,
                right_y: f32::from(*right_y as i8) / 127.0,
                triggers: [*left_trigger, *right_trigger],
                dpad: (flags >> 3) & 0x0f,
                push: flags & 1 != 0,
                push_left: flags & 2 != 0,
                brake: flags & 4 != 0,
                dismount: flags & 0x80 != 0,
                session_modifier: modifiers & 1 != 0,
                respawn: false,
                shove: false,
            }),
            [
                7,
                flags,
                left_trigger,
                right_trigger,
                forward,
                steer,
                right_x,
                right_y,
                modifiers,
            ] if modifiers & !3 == 0 => Some(Self {
                forward: f32::from(*forward as i8) / 127.0,
                steer: f32::from(*steer as i8) / 127.0,
                right_x: f32::from(*right_x as i8) / 127.0,
                right_y: f32::from(*right_y as i8) / 127.0,
                triggers: [*left_trigger, *right_trigger],
                dpad: (flags >> 3) & 0x0f,
                push: flags & 1 != 0,
                push_left: flags & 2 != 0,
                brake: flags & 4 != 0,
                dismount: flags & 0x80 != 0,
                session_modifier: modifiers & 1 != 0,
                respawn: modifiers & 2 != 0,
                shove: false,
            }),
            [
                8,
                flags,
                left_trigger,
                right_trigger,
                forward,
                steer,
                right_x,
                right_y,
                modifiers,
            ] if modifiers & !7 == 0 => Some(Self {
                forward: f32::from(*forward as i8) / 127.0,
                steer: f32::from(*steer as i8) / 127.0,
                right_x: f32::from(*right_x as i8) / 127.0,
                right_y: f32::from(*right_y as i8) / 127.0,
                triggers: [*left_trigger, *right_trigger],
                dpad: (flags >> 3) & 0x0f,
                push: flags & 1 != 0,
                push_left: flags & 2 != 0,
                brake: flags & 4 != 0,
                dismount: flags & 0x80 != 0,
                session_modifier: modifiers & 1 != 0,
                respawn: modifiers & 2 != 0,
                shove: modifiers & 4 != 0,
            }),
            _ => None,
        }
    }
}

/// Control sequence carried by protocol v9. Sequence values wrap modulo 2^32.
pub fn input_sequence(packet: &[u8]) -> Option<u32> {
    if packet.len() != 13 || packet[0] != 9 {
        return None;
    }
    Some(u32::from_be_bytes(packet[9..13].try_into().ok()?))
}

/// Live v5 sessions require sequenced v9 controls; unsequenced legacy packets
/// remain decodable for format tests but are never accepted by the server.
pub fn decode_sequenced(packet: &[u8]) -> Option<(Controls, u32)> {
    let sequence = input_sequence(packet)?;
    Some((Controls::decode(packet)?, sequence))
}

#[cfg(test)]
mod tests {
    use super::{Controls, decode_sequenced, input_sequence};

    #[test]
    fn rejects_malformed_or_unknown_control_packets() {
        assert!(Controls::decode(b"\x01\x00\x00").is_none());
        assert!(Controls::decode(b"\x01\x80\x00\x00").is_none());
        assert!(Controls::decode(b"\x02\x00\x00\x00").is_none());
        assert!(Controls::decode(b"\x02\x80\x00\x00\x00\x00").is_none());
        assert!(Controls::decode(b"\x03\x80\x00\x00\x00\x00\x00\x00").is_none());
        assert!(Controls::decode(b"\x04\x80\x00\x00\x00\x00\x00\x00").is_none());
    }

    #[test]
    fn decodes_v1_axes_and_action_bits() {
        let controls = Controls::decode(&[1, 0b1_1111, 127, 64]).unwrap();
        assert_eq!(controls.forward, 1.0);
        assert_eq!(controls.steer, 64.0 / 127.0);
        assert!(controls.push && controls.brake);
        assert_eq!(controls.right_x, 1.0);
        assert_eq!(controls.right_y, -1.0);
    }

    #[test]
    fn decodes_v2_skate_controls_and_flick_it_axes() {
        let controls = Controls::decode(&[2, 0b111, 0, 64, -127_i8 as u8, 127]).unwrap();
        assert_eq!(controls.steer, 64.0 / 127.0);
        assert_eq!(controls.right_x, -1.0);
        assert_eq!(controls.right_y, 1.0);
        assert!(controls.push && controls.push_left && controls.brake);
        assert_eq!(controls.triggers, [0; 2]);
    }

    #[test]
    fn decodes_v3_grab_triggers_and_flick_it_axes() {
        let controls = Controls::decode(&[3, 1, 192, 255, 0, 0, 64, -127_i8 as u8]).unwrap();
        assert!(controls.push);
        assert_eq!(controls.triggers, [192, 255]);
        assert_eq!(controls.dpad, 0);
        assert_eq!(controls.right_x, 64.0 / 127.0);
        assert_eq!(controls.right_y, -1.0);
    }

    #[test]
    fn decodes_v4_dpad_directions_and_grab_triggers() {
        let controls = Controls::decode(&[4, 0b0101_0010, 64, 192, 0, 0, 0, 0]).unwrap();
        assert_eq!(controls.dpad, 0b1010);
        assert_eq!(controls.triggers, [64, 192]);
    }

    #[test]
    fn decodes_v5_dismount_toggle_and_dpad_with_reserved_flags_clear() {
        let controls = Controls::decode(&[5, 0b1000_1010, 0, 0, 0, 0, 0, 0]).unwrap();
        assert!(controls.dismount);
        assert_eq!(controls.dpad, 1);
    }

    #[test]
    fn decodes_v6_session_marker_modifier_and_rejects_reserved_modifier_bits() {
        let controls = Controls::decode(&[6, 0b1000_1010, 0, 0, 0, 0, 0, 0, 1]).unwrap();
        assert!(controls.session_modifier);
        assert!(controls.dismount);
        assert_eq!(controls.dpad, 1);
        assert!(Controls::decode(&[6, 0, 0, 0, 0, 0, 0, 0, 2]).is_none());
    }

    #[test]
    fn decodes_v7_manual_respawn_action() {
        let controls = Controls::decode(&[7, 0, 0, 0, 0, 0, 0, 0, 3]).unwrap();
        assert!(controls.session_modifier);
        assert!(controls.respawn);
        assert!(!controls.shove);
        assert!(Controls::decode(&[7, 0, 0, 0, 0, 0, 0, 0, 4]).is_none());
    }

    #[test]
    fn decodes_v8_shove_action_and_rejects_reserved_modifier_bits() {
        let controls = Controls::decode(&[8, 0, 0, 0, 0, 0, 0, 0, 7]).unwrap();
        assert!(controls.session_modifier);
        assert!(controls.respawn);
        assert!(controls.shove);
        assert!(Controls::decode(&[8, 0, 0, 0, 0, 0, 0, 0, 8]).is_none());
    }

    #[test]
    fn decodes_v9_controls_and_big_endian_sequence() {
        let packet = [9, 0, 0, 0, 0, 0, 0, 0, 7, 0x12, 0x34, 0x56, 0x78];
        assert!(Controls::decode(&packet).unwrap().shove);
        assert_eq!(input_sequence(&packet), Some(0x1234_5678));
        assert_eq!(decode_sequenced(&packet).unwrap().1, 0x1234_5678);
        assert!(Controls::decode(&packet[..12]).is_none());
        assert_eq!(input_sequence(&packet[..12]), None);
        assert!(decode_sequenced(&[8, 0, 0, 0, 0, 0, 0, 0, 0]).is_none());
    }
}
