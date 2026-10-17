package dev.pexskate.client;

import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;

final class SkatePosePredictionTest {
    @AfterEach
    void resetPredictionLead() {
        SkatePoseCache.resetInputRoundTrip();
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
}
