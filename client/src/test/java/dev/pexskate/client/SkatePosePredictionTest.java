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
    void localCameraHeadingPredictionFollowsRecentAngularMotion() {
        float predicted = SkatePoseCache.Pose.predictAngle(0.0f, 0.2f, 1.0f, 0.05f, 0.025f);

        assertTrue(predicted > 0.2f);
        assertTrue(predicted < 0.4f);
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
