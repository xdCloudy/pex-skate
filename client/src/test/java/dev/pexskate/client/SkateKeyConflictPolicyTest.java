package dev.pexskate.client;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.*;

class SkateKeyConflictPolicyTest {
    @Test
    void suppressesOnlyAnActiveOverlappingAction() {
        assertTrue(SkateKeyConflictPolicy.shouldSuppress(true, true, true));
        assertFalse(SkateKeyConflictPolicy.shouldSuppress(false, true, true));
        assertFalse(SkateKeyConflictPolicy.shouldSuppress(true, false, true));
        assertFalse(SkateKeyConflictPolicy.shouldSuppress(true, true, false));
    }
}
