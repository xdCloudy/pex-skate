package dev.pexskate.client;

import java.util.HashMap;
import java.util.Map;

/** Interpolated, server-authoritative Skate 3 joint positions keyed by entity id. */
public final class SkatePoseCache {
    private static final long INTERPOLATION_DELAY_NANOS = 75_000_000L;
    private static final long LOCAL_INTERPOLATION_DELAY_NANOS = 25_000_000L;
    private static final long MAX_EXTRAPOLATION_NANOS = 50_000_000L;
    private static final Map<Integer, Track> TRACKS = new HashMap<>();

    private SkatePoseCache() {}

    public static void accept(SkateStatePayload packet) {
        if (!packet.active() || packet.boneMask() == 0) {
            TRACKS.remove(packet.entityId());
            return;
        }
        long now = System.nanoTime();
        Track track = TRACKS.get(packet.entityId());
        float[] camera = {
                packet.cameraOffsetX(), packet.cameraOffsetY(), packet.cameraOffsetZ(),
                packet.cameraForwardX(), packet.cameraForwardY(), packet.cameraForwardZ(),
                packet.cameraUpX(), packet.cameraUpY(), packet.cameraUpZ(), packet.cameraFov()
        };
        Frame next = new Frame(now, packet.x(), packet.y(), packet.z(),
                packet.hipsX(), packet.hipsY(), packet.hipsZ(),
                packet.velocityX(), packet.velocityY(), packet.velocityZ(),
                packet.boneMask(), packet.boneOffsets(), packet.boneRotations(), boardYaw(packet), camera);
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
        return sample(entityId, false);
    }

    /** Local presentation uses a shorter buffer and predicts only from the server's latest velocity. */
    public static Pose sample(int entityId, boolean localEntity) {
        Track track = TRACKS.get(entityId);
        if (track == null) return null;
        Frame a = track.previous;
        Frame b = track.current;
        long delay = localEntity ? LOCAL_INTERPOLATION_DELAY_NANOS : INTERPOLATION_DELAY_NANOS;
        long sampleTime = System.nanoTime() - delay;
        if (b.time <= a.time) return new Pose(b, b, 1.0f, 0.0f);
        if (sampleTime >= b.time) {
            long extrapolation = localEntity
                    ? Math.min(sampleTime - b.time, MAX_EXTRAPOLATION_NANOS) : 0L;
            return new Pose(b, b, 1.0f, extrapolation / 1_000_000_000.0f);
        }
        if (sampleTime <= a.time) return new Pose(a, a, 0.0f);
        return new Pose(a, b, (float) (sampleTime - a.time) / (float) (b.time - a.time), 0.0f);
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

    private record Frame(long time, float rootX, float rootY, float rootZ,
                         float hipsX, float hipsY, float hipsZ,
                         float velocityX, float velocityY, float velocityZ,
                         long mask, short[] offsets, short[] rotations, float yaw, float[] camera) {}

    public static final class Pose {
        private final Frame a;
        private final Frame b;
        private final float t;
        private final float extrapolationSeconds;

        private Pose(Frame a, Frame b, float t) { this(a, b, t, 0.0f); }

        private Pose(Frame a, Frame b, float t, float extrapolationSeconds) {
            this.a = a;
            this.b = b;
            this.t = t;
            this.extrapolationSeconds = extrapolationSeconds;
        }

        public boolean has(int index) {
            return ((a.mask & b.mask) & (1L << index)) != 0;
        }

        public float[] point(int index) {
            if (!has(index)) return null;
            float[] first = point(a, index);
            float[] second = point(b, index);
            return new float[] {
                    lerp(first[0], second[0], t) + b.velocityX * extrapolationSeconds,
                    lerp(first[1], second[1], t) + b.velocityY * extrapolationSeconds,
                    lerp(first[2], second[2], t) + b.velocityZ * extrapolationSeconds
            };
        }

        /** World-space x/y/z/w rotation for a Skate rig joint. */
        public float[] rotation(int index) {
            if (!has(index)) return null;
            float[] first = rotation(a, index);
            float[] second = rotation(b, index);
            float dot = first[0] * second[0] + first[1] * second[1]
                    + first[2] * second[2] + first[3] * second[3];
            if (dot < 0.0f) {
                for (int i = 0; i < 4; i++) second[i] = -second[i];
            }
            float[] result = new float[4];
            float magnitude = 0.0f;
            for (int i = 0; i < 4; i++) {
                result[i] = first[i] + (second[i] - first[i]) * t;
                magnitude += result[i] * result[i];
            }
            magnitude = (float) Math.sqrt(magnitude);
            if (magnitude < 1.0e-5f) return new float[] {0.0f, 0.0f, 0.0f, 1.0f};
            for (int i = 0; i < 4; i++) result[i] /= magnitude;
            return result;
        }

        public float yaw() {
            float delta = (b.yaw - a.yaw + (float) Math.PI * 3.0f) % ((float) Math.PI * 2.0f) - (float) Math.PI;
            return a.yaw + delta * t;
        }

        public boolean hasCamera() {
            return Float.isFinite(b.camera[9]) && b.camera[9] >= 30.0f && b.camera[9] <= 120.0f
                    && length(cameraForward()) > 0.5f && length(cameraUp()) > 0.5f;
        }

        public float[] cameraPosition() {
            if (!hasCamera()) return null;
            return new float[] {
                    lerp(a.rootX + a.camera[0], b.rootX + b.camera[0], t) + b.velocityX * extrapolationSeconds,
                    lerp(a.rootY + a.camera[1], b.rootY + b.camera[1], t) + b.velocityY * extrapolationSeconds,
                    lerp(a.rootZ + a.camera[2], b.rootZ + b.camera[2], t) + b.velocityZ * extrapolationSeconds
            };
        }

        public float[] cameraForward() { return vector(3); }
        public float[] cameraUp() { return vector(6); }
        public float cameraFov() { return lerp(a.camera[9], b.camera[9], t); }

        private float[] vector(int offset) {
            if (b.camera.length < offset + 3) return new float[3];
            float[] value = new float[] {
                    lerp(a.camera[offset], b.camera[offset], t),
                    lerp(a.camera[offset + 1], b.camera[offset + 1], t),
                    lerp(a.camera[offset + 2], b.camera[offset + 2], t)
            };
            float length = length(value);
            if (length > 1.0e-5f) {
                value[0] /= length; value[1] /= length; value[2] /= length;
            }
            return value;
        }

        private static float length(float[] value) {
            return (float) Math.sqrt(value[0] * value[0] + value[1] * value[1] + value[2] * value[2]);
        }

        private static float[] point(Frame frame, int index) {
            int i = index * 3;
            return new float[] {
                    frame.hipsX + frame.offsets[i] / 4096.0f,
                    frame.hipsY + frame.offsets[i + 1] / 4096.0f,
                    frame.hipsZ + frame.offsets[i + 2] / 4096.0f
            };
        }

        private static float[] rotation(Frame frame, int index) {
            int i = index * 4;
            float[] result = new float[4];
            float magnitude = 0.0f;
            for (int axis = 0; axis < 4; axis++) {
                result[axis] = frame.rotations[i + axis] / (float) Short.MAX_VALUE;
                magnitude += result[axis] * result[axis];
            }
            magnitude = (float) Math.sqrt(magnitude);
            if (magnitude < 1.0e-5f) return new float[] {0.0f, 0.0f, 0.0f, 1.0f};
            for (int axis = 0; axis < 4; axis++) result[axis] /= magnitude;
            return result;
        }

        private static float lerp(float a, float b, float t) { return a + (b - a) * t; }
    }
}
