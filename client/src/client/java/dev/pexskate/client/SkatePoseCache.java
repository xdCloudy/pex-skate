package dev.pexskate.client;

import java.util.HashMap;
import java.util.Map;

/** Interpolated, server-authoritative Skate 3 joint positions keyed by entity id. */
public final class SkatePoseCache {
    private static final long INTERPOLATION_DELAY_NANOS = 75_000_000L;
    private static final Map<Integer, Track> TRACKS = new HashMap<>();

    private SkatePoseCache() {}

    public static void accept(SkateStatePayload packet) {
        if (!packet.active() || packet.boneMask() == 0) {
            TRACKS.remove(packet.entityId());
            return;
        }
        long now = System.nanoTime();
        Track track = TRACKS.get(packet.entityId());
        Frame next = new Frame(now, packet.hipsX(), packet.hipsY(), packet.hipsZ(),
                packet.boneMask(), packet.boneOffsets(), boardYaw(packet));
        if (track == null) TRACKS.put(packet.entityId(), new Track(next, next));
        else {
            track.previous = track.current;
            track.current = next;
        }
    }

    public static void remove(int entityId) {
        TRACKS.remove(entityId);
    }

    public static void clear() {
        TRACKS.clear();
    }

    public static Pose sample(int entityId) {
        Track track = TRACKS.get(entityId);
        if (track == null) return null;
        Frame a = track.previous;
        Frame b = track.current;
        long sampleTime = System.nanoTime() - INTERPOLATION_DELAY_NANOS;
        if (b.time <= a.time || sampleTime >= b.time) return new Pose(b, b, 1.0f);
        if (sampleTime <= a.time) return new Pose(a, a, 0.0f);
        return new Pose(a, b, (float) (sampleTime - a.time) / (float) (b.time - a.time));
    }

    private static float boardYaw(SkateStatePayload p) {
        float x = p.boardRotX(), y = p.boardRotY(), z = p.boardRotZ(), w = p.boardRotW();
        return (float) Math.atan2(2.0f * (w * y + x * z), 1.0f - 2.0f * (y * y + z * z));
    }

    private static final class Track {
        private Frame previous;
        private Frame current;

        private Track(Frame previous, Frame current) {
            this.previous = previous;
            this.current = current;
        }
    }

    private record Frame(long time, float hipsX, float hipsY, float hipsZ,
                         long mask, short[] offsets, float yaw) {}

    public static final class Pose {
        private final Frame a;
        private final Frame b;
        private final float t;

        private Pose(Frame a, Frame b, float t) { this.a = a; this.b = b; this.t = t; }

        public boolean has(int index) {
            return ((a.mask & b.mask) & (1L << index)) != 0;
        }

        public float[] point(int index) {
            if (!has(index)) return null;
            float[] first = point(a, index);
            float[] second = point(b, index);
            return new float[] {
                    lerp(first[0], second[0], t),
                    lerp(first[1], second[1], t),
                    lerp(first[2], second[2], t)
            };
        }

        public float yaw() {
            float delta = (b.yaw - a.yaw + (float) Math.PI * 3.0f) % ((float) Math.PI * 2.0f) - (float) Math.PI;
            return a.yaw + delta * t;
        }

        private static float[] point(Frame frame, int index) {
            int i = index * 3;
            return new float[] {
                    frame.hipsX + frame.offsets[i] / 4096.0f,
                    frame.hipsY + frame.offsets[i + 1] / 4096.0f,
                    frame.hipsZ + frame.offsets[i + 2] / 4096.0f
            };
        }

        private static float lerp(float a, float b, float t) { return a + (b - a) * t; }
    }
}
