package dev.pexskate.client;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

class SkateToggleRequestGateTest {
    @Test
    void ignoresRepeatedToggleClicksUntilServerAcknowledgesTheChange() {
        SkateToggleRequestGate gate = new SkateToggleRequestGate();

        assertTrue(gate.begin(1_000L));
        assertFalse(gate.begin(1_001L));
        assertFalse(gate.begin(20_000_000_000L));

        gate.acknowledge();
        assertTrue(gate.begin(20_000_000_001L));
    }

    @Test
    void allowsRetryIfTheServerDoesNotAnswerWithinTheTimeout() {
        SkateToggleRequestGate gate = new SkateToggleRequestGate();

        assertTrue(gate.begin(10L));
        assertFalse(gate.begin(30_000_000_009L));
        assertTrue(gate.begin(30_000_000_010L));

        gate.reset();
        assertTrue(gate.begin(30_000_000_011L));
    }
}
