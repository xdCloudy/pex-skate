//! Small deterministic server-side skate controller.
//!
//! This is an adapter for Minecraft movement, not a vendored copy of the
//! standalone Bevy Skate 3 reimplementation.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    pub forward: f32,
    pub steer: f32,
    pub push: bool,
    pub brake: bool,
    pub pop: bool,
    pub flip: bool,
    pub shove: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkateState {
    pub velocity_x: f64,
    pub velocity_z: f64,
    pub yaw: f32,
    pub board_pitch: f32,
    pub board_roll: f32,
    pub score: u32,
    combo_score: u32,
    combo_tricks: u8,
    airborne: bool,
    previous_pop: bool,
    previous_flip: bool,
    previous_shove: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TickResult {
    pub velocity_x: f64,
    pub velocity_y: f64,
    pub velocity_z: f64,
    pub jumped: bool,
    pub trick: Option<Trick>,
    pub landed_score: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Trick {
    Kickflip,
    ShoveIt,
    VarialKickflip,
}

impl Controls {
    /// Decode v1 `pexskate:control` payloads. Axes are signed bytes, flags are
    /// push/brake/pop/flip/shove in bits 0 through 4.
    pub fn decode(packet: &[u8]) -> Option<Self> {
        if packet.len() != 4 || packet[0] != 1 {
            return None;
        }
        let flags = packet[1];
        if flags & !0x1f != 0 {
            return None;
        }
        Some(Self {
            forward: f32::from(packet[2] as i8) / 127.0,
            steer: f32::from(packet[3] as i8) / 127.0,
            push: flags & 1 != 0,
            brake: flags & 2 != 0,
            pop: flags & 4 != 0,
            flip: flags & 8 != 0,
            shove: flags & 16 != 0,
        })
    }
}

impl SkateState {
    /// Advance the server-owned horizontal board state by one Minecraft tick.
    /// Minecraft remains responsible for block collision and gravity.
    pub fn tick(&mut self, controls: Controls, player_yaw: f32, grounded: bool) -> TickResult {
        let forward_input = controls.forward.clamp(-1.0, 1.0) as f64;
        let steer = controls.steer.clamp(-1.0, 1.0);
        if self.speed() < 0.01 {
            self.yaw = player_yaw.to_radians().rem_euclid(std::f32::consts::TAU);
        }
        let yaw_delta = f64::from(steer) * (0.025 + self.speed() * 0.035);
        self.yaw = (self.yaw + yaw_delta as f32).rem_euclid(std::f32::consts::TAU);

        let yaw = f64::from(self.yaw);
        let forward_x = -yaw.sin();
        let forward_z = yaw.cos();
        let acceleration = if controls.push { 0.045 } else { 0.0 };
        self.velocity_x += forward_x * forward_input * acceleration;
        self.velocity_z += forward_z * forward_input * acceleration;

        if controls.brake {
            self.velocity_x *= 0.72;
            self.velocity_z *= 0.72;
        } else {
            self.velocity_x *= 0.992;
            self.velocity_z *= 0.992;
        }

        let speed = self.speed();
        let max_speed = 0.78;
        if speed > max_speed {
            self.velocity_x *= max_speed / speed;
            self.velocity_z *= max_speed / speed;
        }

        let jumped = grounded && controls.pop && !self.previous_pop;
        let flip_pressed = controls.flip && !self.previous_flip;
        let shove_pressed = controls.shove && !self.previous_shove;
        let trick = if !grounded && flip_pressed && shove_pressed {
            Some(Trick::VarialKickflip)
        } else if !grounded && flip_pressed {
            Some(Trick::Kickflip)
        } else if !grounded && shove_pressed {
            Some(Trick::ShoveIt)
        } else {
            None
        };

        if let Some(trick) = trick {
            let points = match trick {
                Trick::Kickflip => 100,
                Trick::ShoveIt => 150,
                Trick::VarialKickflip => 250,
            };
            let link_bonus = u32::from(self.combo_tricks).saturating_mul(25);
            self.combo_score = self.combo_score.saturating_add(points + link_bonus);
            self.combo_tricks = self.combo_tricks.saturating_add(1);
        }

        let landed_score = if !grounded {
            self.airborne = true;
            None
        } else if self.airborne {
            self.airborne = false;
            let landed_score = (self.combo_score > 0).then_some(self.combo_score);
            if let Some(points) = landed_score {
                self.score = self.score.saturating_add(points);
            }
            self.combo_score = 0;
            self.combo_tricks = 0;
            landed_score
        } else {
            None
        };

        if !grounded && controls.flip {
            self.board_roll += 0.32;
        } else if !grounded && controls.shove {
            self.board_roll += 0.2;
        } else if grounded {
            self.board_pitch = 0.0;
            self.board_roll *= 0.62;
        } else {
            self.board_roll *= 0.9;
        }

        self.previous_pop = controls.pop;
        self.previous_flip = controls.flip;
        self.previous_shove = controls.shove;

        TickResult {
            velocity_x: self.velocity_x,
            velocity_y: if jumped { 0.42 } else { 0.0 },
            velocity_z: self.velocity_z,
            jumped,
            trick,
            landed_score,
        }
    }

    pub fn speed(&self) -> f64 {
        self.velocity_x.hypot(self.velocity_z)
    }
}

#[cfg(test)]
mod tests {
    use super::{Controls, SkateState, Trick};

    #[test]
    fn rejects_malformed_or_unknown_control_packets() {
        assert!(Controls::decode(b"\x01\x00\x00").is_none());
        assert!(Controls::decode(b"\x01\x80\x00\x00").is_none());
        assert!(Controls::decode(b"\x02\x00\x00\x00").is_none());
    }

    #[test]
    fn pushing_accelerates_and_braking_slows_down() {
        let mut skate = SkateState::default();
        for _ in 0..20 {
            skate.tick(
                Controls {
                    forward: 1.0,
                    push: true,
                    ..Controls::default()
                },
                0.0,
                true,
            );
        }
        let fast = skate.speed();
        skate.tick(
            Controls {
                brake: true,
                ..Controls::default()
            },
            0.0,
            true,
        );
        assert!(fast > skate.speed());
    }

    #[test]
    fn pop_is_edge_triggered_and_air_tricks_score_when_landed() {
        let mut skate = SkateState::default();
        assert!(
            skate
                .tick(
                    Controls {
                        pop: true,
                        ..Controls::default()
                    },
                    0.0,
                    true
                )
                .jumped
        );
        assert!(
            !skate
                .tick(
                    Controls {
                        pop: true,
                        ..Controls::default()
                    },
                    0.0,
                    true
                )
                .jumped
        );
        assert_eq!(
            skate
                .tick(
                    Controls {
                        flip: true,
                        ..Controls::default()
                    },
                    0.0,
                    false
                )
                .trick,
            Some(Trick::Kickflip)
        );
        assert_eq!(skate.score, 0, "airborne tricks are not banked yet");
        assert_eq!(
            skate
                .tick(Controls::default(), 0.0, true)
                .landed_score,
            Some(100)
        );
        assert_eq!(skate.score, 100);
    }

    #[test]
    fn linked_tricks_add_a_combo_bonus_and_only_bank_on_landing() {
        let mut skate = SkateState::default();
        let kickflip = Controls { flip: true, ..Controls::default() };
        let shove = Controls { shove: true, ..Controls::default() };
        assert_eq!(skate.tick(kickflip, 0.0, false).trick, Some(Trick::Kickflip));
        skate.tick(Controls::default(), 0.0, false);
        assert_eq!(skate.tick(shove, 0.0, false).trick, Some(Trick::ShoveIt));
        assert_eq!(skate.tick(Controls::default(), 0.0, true).landed_score, Some(275));
        assert_eq!(skate.score, 275);
    }
}
