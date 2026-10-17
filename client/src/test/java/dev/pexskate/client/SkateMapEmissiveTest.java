package dev.pexskate.client;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;

class SkateMapEmissiveTest {
    @Test
    void unlitMaterialsKeepTheirAlbedoAtZeroWorldLight() {
        assertEquals(0.6f, SkateMapModel.emissiveMix(0.0f, 0.6f, 1.0f), 0.001f);
    }

    @Test
    void nonEmissiveMaterialsKeepTheirNormalLitValue() {
        assertEquals(0.2f, SkateMapModel.emissiveMix(0.2f, 0.8f, 0.0f), 0.001f);
    }

    @Test
    void partialEmissionBlendsLitAndUnlitContributions() {
        assertEquals(0.5f, SkateMapModel.emissiveMix(0.2f, 0.8f, 0.5f), 0.001f);
    }
}
