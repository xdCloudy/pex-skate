package dev.pexskate.client;

import net.minecraft.world.phys.Vec3;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

class SkateCameraCollisionTest {
    @Test
    void segmentStopsAtTriangleIntersection() {
        double fraction = SkateMapModel.segmentTriangleFraction(
                new Vec3(0.0, 0.0, 0.0), new Vec3(0.0, 0.0, 4.0),
                -1.0, -1.0, 2.0,
                1.0, -1.0, 2.0,
                0.0, 1.0, 2.0,
                1.0);

        assertEquals(0.5, fraction, 0.0001);
    }

    @Test
    void segmentIgnoresTrianglesOutsideItsBoundsAndMaximumFraction() {
        Vec3 from = new Vec3(0.0, 0.0, 0.0);
        Vec3 to = new Vec3(0.0, 0.0, 4.0);

        double outside = SkateMapModel.segmentTriangleFraction(from, to,
                3.0, -1.0, 2.0,
                4.0, -1.0, 2.0,
                3.0, 1.0, 2.0,
                1.0);
        double beyond = SkateMapModel.segmentTriangleFraction(from, to,
                -1.0, -1.0, 3.0,
                1.0, -1.0, 3.0,
                0.0, 1.0, 3.0,
                0.5);

        assertTrue(Double.isInfinite(outside));
        assertTrue(Double.isInfinite(beyond));
    }
}
