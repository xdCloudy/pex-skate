package dev.pexskate.client;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;

class SkateMapNormalMapTest {
    @Test
    void flatNormalMapPreservesTheAuthoredVertexNormal() {
        float[] normals = {0.0f, 1.0f, 0.0f};
        SkateMapModel.TextureData flat = new SkateMapModel.TextureData(
                "flat-normal", 1, 1, new byte[] {(byte) 128, (byte) 128, (byte) 255, (byte) 255});

        SkateMapModel.applyVertexNormalMap(normals, 0, 0.5f, 0.5f, flat,
                (byte) 127, (byte) 0, (byte) 0, (byte) 127);

        assertEquals(0.0f, normals[0], 0.01f);
        assertEquals(1.0f, normals[1], 0.01f);
        assertEquals(0.0f, normals[2], 0.01f);
    }

    @Test
    void zeroHandednessLeavesTheAuthoredVertexNormalUntouched() {
        float[] normals = {0.0f, 1.0f, 0.0f};
        SkateMapModel.TextureData tilted = new SkateMapModel.TextureData(
                "tilted-normal", 1, 1, new byte[] {(byte) 255, (byte) 128, (byte) 128, (byte) 255});

        SkateMapModel.applyVertexNormalMap(normals, 0, 0.5f, 0.5f, tilted,
                (byte) 0, (byte) 0, (byte) 0, (byte) 0);

        assertEquals(0.0f, normals[0]);
        assertEquals(1.0f, normals[1]);
        assertEquals(0.0f, normals[2]);
    }

    @Test
    void tangentSpaceDirectionUsesTheExportedBinormalAndHandedness() {
        float[] normals = {0.0f, 1.0f, 0.0f};
        SkateMapModel.TextureData tangentFacing = new SkateMapModel.TextureData(
                "tangent-facing", 1, 1, new byte[] {(byte) 255, (byte) 128, (byte) 128, (byte) 255});

        SkateMapModel.applyVertexNormalMap(normals, 0, 0.5f, 0.5f, tangentFacing,
                (byte) 127, (byte) 0, (byte) 0, (byte) 127);

        assertEquals(0.0f, normals[0], 0.01f);
        assertEquals(0.0f, normals[1], 0.01f);
        assertEquals(1.0f, normals[2], 0.01f);
    }
}
