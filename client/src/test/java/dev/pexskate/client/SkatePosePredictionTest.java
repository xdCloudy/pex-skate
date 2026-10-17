package dev.pexskate.client;

import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;

import java.util.List;

import static org.junit.jupiter.api.Assertions.assertNotSame;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

final class SkatePosePredictionTest {
    @AfterEach
    void resetPredictionLead() {
        SkatePoseCache.resetInputRoundTrip();
        SkatePoseCache.clear();
    }

    @Test
    void appliesHalfRoundTripAsSoonAsASnapshotArrives() {
        SkatePoseCache.updateInputRoundTrip(60_000_000L);

        assertEquals(30_000_000L, SkatePoseCache.extrapolationNanos(0L, true));
    }

    @Test
    void predictionAdvancesFromSnapshotArrivalAndStaysBounded() {
        SkatePoseCache.updateInputRoundTrip(60_000_000L);

        assertEquals(80_000_000L, SkatePoseCache.extrapolationNanos(50_000_000L, true));
        assertEquals(100_000_000L, SkatePoseCache.extrapolationNanos(200_000_000L, true));
    }

    @Test
    void remoteSkatersDoNotUseLocalInputPrediction() {
        SkatePoseCache.updateInputRoundTrip(60_000_000L);

        assertEquals(0L, SkatePoseCache.extrapolationNanos(50_000_000L, false));
    }

    @Test
    void localSkaterRenderReusesTheCameraPoseUntilTheNextCameraSample() {
        SkatePoseCache.accept(state(42, 1.0f));
        SkatePoseCache.Pose cameraPose = SkatePoseCache.sampleForRenderCamera(42);

        SkatePoseCache.accept(state(42, 2.0f));
        assertSame(cameraPose, SkatePoseCache.sample(42, true));

        SkatePoseCache.Pose nextFramePose = SkatePoseCache.sampleForRenderCamera(42);
        assertNotSame(cameraPose, nextFramePose);
        assertSame(nextFramePose, SkatePoseCache.sample(42, true));
    }

    @Test
    void sampledRenderRootUsesTheSameAuthoritativePositionAsTheCameraAnchor() {
        SkatePoseCache.accept(state(42, 1.0f));
        SkatePoseCache.Pose pose = SkatePoseCache.sampleForRenderCamera(42);

        assertEquals(1.0f, pose.rootPosition()[0]);
        assertEquals(0.0f, pose.rootPosition()[1]);
        assertEquals(0.0f, pose.rootPosition()[2]);
        assertEquals(pose.point(1)[0], pose.rootPosition()[0]);
    }

    @Test
    void localCameraHeadingPredictionFollowsRecentAngularMotion() {
        float predicted = SkatePoseCache.Pose.predictAngle(0.0f, 0.2f, 1.0f, 0.05f, 0.025f);

        assertTrue(predicted > 0.2f);
        assertTrue(predicted < 0.4f);
    }

    @Test
    void localSkeletalAnchorPredictionAdvancesAlongWithTheRenderedRoot() {
        float[] predicted = SkatePoseCache.Pose.predictPoint(
                new float[] {1.0f, 2.0f, 3.0f},
                new float[] {2.0f, 2.5f, 4.0f},
                1.0f,
                0.05f,
                0.025f,
                new float[] {0.0f, 0.0f, 0.0f});

        assertEquals(2.5f, predicted[0], 1.0e-5f);
        assertEquals(2.75f, predicted[1], 1.0e-5f);
        assertEquals(4.5f, predicted[2], 1.0e-5f);
    }

    @Test
    void localSkeletalAnchorPredictionUsesRootVelocityWithoutTwoSnapshots() {
        float[] predicted = SkatePoseCache.Pose.predictPoint(
                new float[] {1.0f, 2.0f, 3.0f},
                new float[] {1.0f, 2.0f, 3.0f},
                1.0f,
                0.0f,
                0.025f,
                new float[] {4.0f, 0.0f, -2.0f});

        assertEquals(1.1f, predicted[0], 1.0e-5f);
        assertEquals(2.0f, predicted[1], 1.0e-5f);
        assertEquals(2.95f, predicted[2], 1.0e-5f);
    }

    @Test
    void closeSnapshotArrivalsDoNotAmplifyJointMotionIntoACameraJump() {
        float[] predicted = SkatePoseCache.Pose.predictPoint(
                new float[] {0.0f, 0.0f, 0.0f},
                new float[] {0.1f, 0.0f, 0.0f},
                1.0f,
                0.001f,
                0.025f,
                new float[] {0.0f, 0.0f, 0.0f});

        assertEquals(0.2f, predicted[0], 1.0e-5f);
    }

    @Test
    void localSkeletalRotationPredictionAdvancesWithTheJointPositions() {
        float[] predicted = SkatePoseCache.Pose.predictRotation(
                new float[] {0.0f, 0.0f, 0.0f, 1.0f},
                new float[] {0.0f, (float) Math.sin(Math.PI / 12.0), 0.0f,
                        (float) Math.cos(Math.PI / 12.0)},
                1.0f,
                0.05f,
                0.025f);
        float predictedAngle = 2.0f * (float) Math.atan2(Math.abs(predicted[1]), predicted[3]);

        assertTrue(predictedAngle > (float) (Math.PI / 6.0));
        assertTrue(predictedAngle < (float) (Math.PI / 3.0));
    }

    @Test
    void closeSnapshotArrivalsDoNotAmplifyJointRotationPrediction() {
        float[] predicted = SkatePoseCache.Pose.predictRotation(
                new float[] {0.0f, 0.0f, 0.0f, 1.0f},
                new float[] {0.0f, (float) Math.sin(Math.PI / 12.0), 0.0f,
                        (float) Math.cos(Math.PI / 12.0)},
                1.0f,
                0.001f,
                0.025f);
        float predictedAngle = 2.0f * (float) Math.atan2(Math.abs(predicted[1]), predicted[3]);

        assertEquals((float) (Math.PI / 3.0), predictedAngle, 1.0e-4f);
    }

    @Test
    void jointRotationPredictionUsesTheShortestQuaternionArc() {
        float[] predicted = SkatePoseCache.Pose.predictRotation(
                new float[] {0.0f, 0.0f, 0.0f, 1.0f},
                new float[] {0.0f, -(float) Math.sin(Math.PI / 12.0), 0.0f,
                        -(float) Math.cos(Math.PI / 12.0)},
                1.0f,
                0.05f,
                0.025f);
        float predictedAngle = 2.0f * (float) Math.atan2(Math.abs(predicted[1]), predicted[3]);

        assertTrue(predicted[1] > 0.0f);
        assertEquals((float) (Math.PI / 4.0), predictedAngle, 1.0e-4f);
    }

    @Test
    void minecraftChaseHeadingUsesTheSkaterBodyRatherThanBoardTrickRotation() {
        SkatePoseCache.accept(state(42, 1.0f));

        float heading = SkatePoseCache.sample(42, true).characterYaw();

        assertEquals(Math.PI, Math.abs(heading), 1.0e-3);
    }

    private static SkateStatePayload state(int entityId, float x) {
        return new SkateStatePayload(
                (byte) 11, true, entityId,
                x, 0.0f, 0.0f,
                x, 0.0f, 0.0f,
                0.0f, 0.0f, 0.0f, 1.0f,
                1.0f, 0.0f, 0.0f,
                0.0f, 0.0f, 0.0f, 1.0f, 0.0f,
                (byte) 0, "",
                x, 0.0f, 0.0f,
                (1L << 1) | (1L << 5) | (1L << 17) | (1L << 21),
                bodyOffsets(), new short[36 * 4],
                0.0f, 0.0f, 0.0f,
                0.0f, 0.0f, 1.0f,
                0.0f, 1.0f, 0.0f,
                70.0f,
                false, "", "", "", 0, List.of());
    }

    private static short[] bodyOffsets() {
        short[] offsets = new short[36 * 3];
        offsets[5 * 3 + 1] = 4096;
        offsets[17 * 3] = 2048;
        offsets[21 * 3] = -2048;
        return offsets;
    }
}
