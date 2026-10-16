//! Versioned client-input protocol. Gameplay simulation is owned by the
//! vendored Skate 3 host, not recreated in this PEX adapter.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    pub forward: f32,
    pub steer: f32,
    pub right_x: f32,
    pub right_y: f32,
    pub push: bool,
    pub push_left: bool,
    pub brake: bool,
}

impl Controls {
    /// Decode the original v1 action packet and v2 controller packet. V2 has
    /// right/left-foot push and brake buttons plus four signed stick axes.
    pub fn decode(packet: &[u8]) -> Option<Self> {
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
                push: flags & 1 != 0,
                push_left: false,
                brake: flags & 2 != 0,
            }),
            [2, flags, forward, steer, right_x, right_y] if flags & !0x07 == 0 => Some(Self {
                forward: f32::from(*forward as i8) / 127.0,
                steer: f32::from(*steer as i8) / 127.0,
                right_x: f32::from(*right_x as i8) / 127.0,
                right_y: f32::from(*right_y as i8) / 127.0,
                push: flags & 1 != 0,
                push_left: flags & 2 != 0,
                brake: flags & 4 != 0,
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Controls;

    #[test]
    fn rejects_malformed_or_unknown_control_packets() {
        assert!(Controls::decode(b"\x01\x00\x00").is_none());
        assert!(Controls::decode(b"\x01\x80\x00\x00").is_none());
        assert!(Controls::decode(b"\x02\x00\x00\x00").is_none());
        assert!(Controls::decode(b"\x02\x80\x00\x00\x00\x00").is_none());
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
    }
}
