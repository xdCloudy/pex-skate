package dev.pexskate.client;

import java.util.HashMap;
import java.util.Map;

/** Interpolated, server-authoritative Skate 3 joint positions keyed by entity id. */
public final class SkatePoseCache {
    private static final long INTERPOLATION_DELAY_NANOS = 75_000_000L;
    // The local player's chase camera and board must follow the newest server
    // pose immediately. A 25 ms interpolation buffer made both feel as though
    // they were trailing the skater during pushes and turns.
    private static final long LOCAL_INTERPOLATION_DELAY_NANOS = 0L;
    private static final long MAX_LOCAL_PREDICTION_NANOS = 100_000_000L;
    private static final long MAX_LOCAL_PREDICTION_LEAD_NANOS = 50_000_000L;
    private static final long DEFAULT_LOCAL_PREDICTION_LEAD_NANOS = 25_000_000L;
    private static final long MAX_INTERPOLATION_GAP_NANOS = 500_000_000L;
    private static final float SNAP_DISTANCE_SQUARED = 64.0f;
    // Server pose snapshots are broadcast at 20 Hz. Client packet tasks can be
    // drained together, so receipt timestamps shorter than one broadcast
    // interval must not be treated as the simulation interval for prediction.
    private static final float MIN_PREDICTION_SAMPLE_SECONDS = 0.050f;
    // The RTT-derived lead compensates for the age of the server snapshot at
    // receipt. It is added to render time since receipt, rather than only
    // limiting that elapsed time; otherwise a fresh packet still renders an
    // already-stale pose with no prediction at all.
    private static volatile long localPredictionLeadNanos = DEFAULT_LOCAL_PREDICTION_LEAD_NANOS;
    private static final Map<Integer, Track> TRACKS = new HashMap<>();
    // Camera extraction runs before entity rendering. Pin its local pose for
    // the rest of that render frame so the camera, vanilla rig, and GLB all
    // use the same network snapshot even if another payload is applied mid-frame.
    private static volatile int renderPoseEntityId = Integer.MIN_VALUE;
    private static volatile Pose renderPose;

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
        else if (isDiscontinuous(track.current, next)) TRACKS.put(packet.entityId(), new Track(next, next));
        else {
            track.previous = track.current;
            track.current = next;
        }
    }

    private static boolean isDiscontinuous(Frame previous, Frame next) {
        float dx = next.rootX - previous.rootX;
        float dy = next.rootY - previous.rootY;
        float dz = next.rootZ - previous.rootZ;
        return next.time - previous.time > MAX_INTERPOLATION_GAP_NANOS
                || dx * dx + dy * dy + dz * dz > SNAP_DISTANCE_SQUARED;
    }

    public static void remove(int entityId) {
        TRACKS.remove(entityId);
        if (renderPoseEntityId == entityId) clearRenderPose();
    }

    public static void clear() {
        TRACKS.clear();
        clearRenderPose();
    }

    public static void updateInputRoundTrip(long roundTripNanos) {
        if (roundTripNanos <= 0L) return;
        localPredictionLeadNanos = Math.clamp(roundTripNanos / 2L,
                5_000_000L, MAX_LOCAL_PREDICTION_LEAD_NANOS);
    }

    public static void resetInputRoundTrip() {
        localPredictionLeadNanos = DEFAULT_LOCAL_PREDICTION_LEAD_NANOS;
    }

    public static Pose sample(int entityId) {
        return sample(entityId, false);
    }

    /** Local presentation uses a shorter buffer and predicts only from the server's latest velocity. */
    public static Pose sample(int entityId, boolean localEntity) {
        if (localEntity && renderPoseEntityId == entityId) return renderPose;
        return sampleUnpinned(entityId, localEntity);
    }

    /** Samples and pins the pose at camera extraction, before any skater render hooks run. */
    public static Pose sampleForRenderCamera(int entityId) {
        Pose pose = sampleUnpinned(entityId, true);
        renderPose = pose;
        renderPoseEntityId = entityId;
        return pose;
    }

    private static Pose sampleUnpinned(int entityId, boolean localEntity) {
        Track track = TRACKS.get(entityId);
        if (track == null) return null;
        Frame a = track.previous;
        Frame b = track.current;
        long delay = localEntity ? LOCAL_INTERPOLATION_DELAY_NANOS : INTERPOLATION_DELAY_NANOS;
        long sampleTime = System.nanoTime() - delay;
        if (b.time <= a.time) return new Pose(b, b, 1.0f, 0.0f);
        if (sampleTime >= b.time) {
            long extrapolation = extrapolationNanos(sampleTime - b.time, localEntity);
            return new Pose(b, b, 1.0f, extrapolation / 1_000_000_000.0f);
        }
        if (sampleTime <= a.time) return new Pose(a, a, 0.0f);
        return new Pose(a, b, (float) (sampleTime - a.time) / (float) (b.time - a.time), 0.0f);
    }

    private static void clearRenderPose() {
        renderPoseEntityId = Integer.MIN_VALUE;
        renderPose = null;
    }

    static long extrapolationNanos(long elapsedSinceSnapshotNanos, boolean localEntity) {
        if (!localEntity) return 0L;
        long elapsed = Math.max(0L, elapsedSinceSnapshotNanos);
        return Math.min(localPredictionLeadNanos + elapsed, MAX_LOCAL_PREDICTION_NANOS);
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
            return predictRootRelativePoint(first, second,
                    rootPositionFor(a), rootPositionFor(b), rootPosition(), t,
                    snapshotDeltaSeconds(), extrapolationSeconds);
        }

        /**
         * Carry every local joint with the exact root prediction used by the
         * Minecraft avatar and chase camera. Predict only articulation in
         * root-relative space so differences between server velocity and the
         * finite-difference velocity of a joint cannot pull the skater away
         * from its camera anchor during acceleration.
         */
        static float[] predictRootRelativePoint(float[] firstPoint, float[] secondPoint,
                                                float[] firstRoot, float[] secondRoot,
                                                float[] predictedRoot, float interpolation,
                                                float snapshotDeltaSeconds, float predictionSeconds) {
            float[] firstRelative = subtract(firstPoint, firstRoot);
            float[] secondRelative = subtract(secondPoint, secondRoot);
            float[] relative = predictPoint(firstRelative, secondRelative, interpolation,
                    snapshotDeltaSeconds, predictionSeconds, new float[3]);
            return new float[] {
                    predictedRoot[0] + relative[0],
                    predictedRoot[1] + relative[1],
                    predictedRoot[2] + relative[2]
            };
        }

        /**
         * Sample a joint between snapshots and carry its observed motion forward
         * with the same prediction interval used for the local render root. The
         * GLB is positioned from these world-space joints, so leaving them at the
         * last server sample made the skater appear to trail a camera whose root
         * position was already being predicted.
         */
        static float[] predictPoint(float[] first, float[] second, float interpolation,
                                    float snapshotDeltaSeconds, float predictionSeconds,
                                    float[] fallbackVelocity) {
            float[] result = new float[3];
            float predictionDelta = predictionSampleDelta(snapshotDeltaSeconds);
            for (int axis = 0; axis < 3; axis++) {
                float sampled = lerp(first[axis], second[axis], interpolation);
                if (predictionDelta > 0.0f && predictionSeconds > 0.0f) {
                    sampled += (second[axis] - first[axis]) / predictionDelta * predictionSeconds;
                } else if (predictionSeconds > 0.0f) {
                    sampled += fallbackVelocity[axis] * predictionSeconds;
                }
                result[axis] = sampled;
            }
            return result;
        }

        /** World-space simulation root used to place Minecraft's avatar render state. */
        public float[] rootPosition() {
            return interpolateRoot(a, b, t, extrapolationSeconds);
        }

        private static float[] rootPositionFor(Frame frame) {
            return new float[] {frame.rootX, frame.rootY, frame.rootZ};
        }

        private static float[] interpolateRoot(Frame first, Frame second, float interpolation,
                                               float predictionSeconds) {
            return new float[] {
                    lerp(first.rootX, second.rootX, interpolation) + second.velocityX * predictionSeconds,
                    lerp(first.rootY, second.rootY, interpolation) + second.velocityY * predictionSeconds,
                    lerp(first.rootZ, second.rootZ, interpolation) + second.velocityZ * predictionSeconds
            };
        }

        /** World-space x/y/z/w rotation for a Skate rig joint. */
        public float[] rotation(int index) {
            if (!has(index)) return null;
            float[] first = rotation(a, index);
            float[] second = rotation(b, index);
            return predictRotation(first, second, t, snapshotDeltaSeconds(), extrapolationSeconds);
        }

        static float[] predictRotation(float[] first, float[] second, float interpolation,
                                       float snapshotDeltaSeconds, float predictionSeconds) {
            float dot = first[0] * second[0] + first[1] * second[1]
                    + first[2] * second[2] + first[3] * second[3];
            if (dot < 0.0f) {
                for (int i = 0; i < 4; i++) second[i] = -second[i];
                dot = -dot;
            }
            float sampledTime = interpolation;
            if (snapshotDeltaSeconds > 0.0f && predictionSeconds > 0.0f) {
                sampledTime += predictionSeconds / predictionSampleDelta(snapshotDeltaSeconds);
            }
            float[] result = new float[4];
            dot = Math.clamp(dot, -1.0f, 1.0f);
            if (dot > 0.9995f) {
                for (int i = 0; i < 4; i++) {
                    result[i] = first[i] + (second[i] - first[i]) * sampledTime;
                }
            } else {
                double angle = Math.acos(dot);
                double sinAngle = Math.sin(angle);
                double firstWeight = Math.sin((1.0 - sampledTime) * angle) / sinAngle;
                double secondWeight = Math.sin(sampledTime * angle) / sinAngle;
                for (int i = 0; i < 4; i++) {
                    result[i] = (float) (first[i] * firstWeight + second[i] * secondWeight);
                }
            }
            float magnitude = 0.0f;
            for (float component : result) magnitude += component * component;
            magnitude = (float) Math.sqrt(magnitude);
            if (magnitude < 1.0e-5f) return new float[] {0.0f, 0.0f, 0.0f, 1.0f};
            for (int i = 0; i < 4; i++) result[i] /= magnitude;
            return result;
        }

        private static float predictionSampleDelta(float snapshotDeltaSeconds) {
            return snapshotDeltaSeconds > 0.0f
                    ? Math.max(snapshotDeltaSeconds, MIN_PREDICTION_SAMPLE_SECONDS)
                    : 0.0f;
        }

        public float yaw() {
            return predictAngle(a.yaw, b.yaw, t, snapshotDeltaSeconds(), extrapolationSeconds);
        }

        /** Horizontal facing direction of the skater's body, independent of board trick rotation. */
        public float characterYaw() {
            float[] hips = point(1);
            float[] spine = point(5);
            float[] leftHip = point(21);
            float[] rightHip = point(17);
            if (hips == null || spine == null || leftHip == null || rightHip == null) return yaw();

            float[] up = subtract(spine, hips);
            float[] right = subtract(rightHip, leftHip);
            float[] forward = cross(up, right);
            float horizontalLength = (float) Math.sqrt(forward[0] * forward[0] + forward[2] * forward[2]);
            if (!Float.isFinite(horizontalLength) || horizontalLength < 1.0e-4f) return yaw();
            return (float) Math.atan2(-forward[0], forward[2]);
        }

        public boolean hasCamera() {
            return Float.isFinite(b.camera[9]) && b.camera[9] >= 30.0f && b.camera[9] <= 120.0f
                    && length(cameraForward()) > 0.5f && length(cameraUp()) > 0.5f;
        }

        public float[] cameraPosition() {
            if (!hasCamera()) return null;
            float[] previous = cameraWorldPosition(a);
            float[] current = cameraWorldPosition(b);
            float[] result = new float[3];
            float dt = snapshotDeltaSeconds();
            for (int axis = 0; axis < 3; axis++) {
                float sampled = lerp(previous[axis], current[axis], t);
                float velocity = dt > 0.0f
                        ? (current[axis] - previous[axis]) / dt
                        : axis == 0 ? b.velocityX : axis == 1 ? b.velocityY : b.velocityZ;
                result[axis] = sampled + velocity * extrapolationSeconds;
            }
            return result;
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
            float dt = snapshotDeltaSeconds();
            if (dt > 0.0f && extrapolationSeconds > 0.0f) {
                float prediction = Math.min(extrapolationSeconds, dt);
                for (int axis = 0; axis < 3; axis++) {
                    value[axis] += (b.camera[offset + axis] - a.camera[offset + axis])
                            / dt * prediction;
                }
            }
            float length = length(value);
            if (length > 1.0e-5f) {
                value[0] /= length; value[1] /= length; value[2] /= length;
            }
            return value;
        }

        private float snapshotDeltaSeconds() {
            return b.time > a.time ? (b.time - a.time) / 1_000_000_000.0f : 0.0f;
        }

        static float predictAngle(float first, float second, float interpolation,
                                  float snapshotDeltaSeconds, float predictionSeconds) {
            float delta = (second - first + (float) Math.PI * 3.0f)
                    % ((float) Math.PI * 2.0f) - (float) Math.PI;
            float sampled = first + delta * interpolation;
            if (snapshotDeltaSeconds > 0.0f) {
                sampled += delta / snapshotDeltaSeconds * Math.min(predictionSeconds, snapshotDeltaSeconds);
            }
            return wrapAngle(sampled);
        }

        private static float[] cameraWorldPosition(Frame frame) {
            return new float[] {
                    frame.rootX + frame.camera[0],
                    frame.rootY + frame.camera[1],
                    frame.rootZ + frame.camera[2]
            };
        }

        private static float wrapAngle(float angle) {
            return (angle + (float) Math.PI) % ((float) Math.PI * 2.0f)
                    - (float) Math.PI;
        }

        private static float length(float[] value) {
            return (float) Math.sqrt(value[0] * value[0] + value[1] * value[1] + value[2] * value[2]);
        }

        private static float[] subtract(float[] first, float[] second) {
            return new float[] {first[0] - second[0], first[1] - second[1], first[2] - second[2]};
        }

        private static float[] cross(float[] first, float[] second) {
            return new float[] {
                    first[1] * second[2] - first[2] * second[1],
                    first[2] * second[0] - first[0] * second[2],
                    first[0] * second[1] - first[1] * second[0]
            };
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
