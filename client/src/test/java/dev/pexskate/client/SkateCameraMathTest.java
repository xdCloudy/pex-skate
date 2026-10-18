package dev.pexskate.client;

import net.minecraft.world.phys.Vec3;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

class SkateCameraMathTest {
    @Test
    void chaseCameraStaysBehindThePoseAtDifferentHeadings() {
        Vec3 root = new Vec3(10.0, 64.0, -3.0);

        Vec3 facingPositiveZ = SkateCameraMath.minecraftChasePosition(root, 0.0f);
        assertEquals(10.0, facingPositiveZ.x, 1.0e-6);
        assertEquals(65.55, facingPositiveZ.y, 1.0e-6);
        assertEquals(-7.0, facingPositiveZ.z, 1.0e-6);

        Vec3 facingNegativeX = SkateCameraMath.minecraftChasePosition(root, (float) (Math.PI / 2.0));
        assertEquals(14.0, facingNegativeX.x, 1.0e-6);
        assertEquals(-3.0, facingNegativeX.z, 1.0e-6);
    }

    @Test
    void chaseLookAtUsesThePlayerRenderRootRatherThanAnimatedHipHeight() {
        Vec3 root = new Vec3(10.0, 64.0, -3.0);

        Vec3 lookAt = SkateCameraMath.minecraftChaseLookAt(root);

        assertEquals(root.x, lookAt.x, 1.0e-6);
        assertEquals(64.75, lookAt.y, 1.0e-6);
        assertEquals(root.z, lookAt.z, 1.0e-6);
    }

    @Test
    void chaseCameraAimsAtItsSkatePoseAnchor() {
        Vec3 camera = new Vec3(0.0, 66.0, -4.0);
        Vec3 target = new Vec3(0.0, 64.75, 0.0);

        SkateCameraMath.Rotation rotation = SkateCameraMath.lookAt(camera, target);
        double pitchRadians = Math.toRadians(rotation.pitch());
        double yawRadians = Math.toRadians(rotation.yaw());
        Vec3 direction = new Vec3(-Math.sin(yawRadians) * Math.cos(pitchRadians),
                -Math.sin(pitchRadians), Math.cos(yawRadians) * Math.cos(pitchRadians));
        Vec3 expected = target.subtract(camera).normalize();

        assertEquals(expected.x, direction.x, 1.0e-6);
        assertEquals(expected.y, direction.y, 1.0e-6);
        assertEquals(expected.z, direction.z, 1.0e-6);
    }

    @Test
    void rejectsNonFiniteCameraAnchorsAndRotationsBeforeTheyReachMinecraftCulling() {
        assertFalse(SkateCameraMath.isFinite(new Vec3(Double.NaN, 64.0, 0.0)));
        assertFalse(SkateCameraMath.isFinite(new Vec3(0.0, Double.POSITIVE_INFINITY, 0.0)));
        assertFalse(SkateCameraMath.isFinite(new SkateCameraMath.Rotation(Float.NaN, 0.0f)));
        assertFalse(SkateCameraMath.isFinite(new SkateCameraMath.Rotation(0.0f, Float.NEGATIVE_INFINITY)));
        assertFalse(SkateCameraMath.isFiniteViewVector(Float.NaN, 0.0f, 1.0f, 0.0f));
        assertFalse(SkateCameraMath.isFiniteViewVector(0.0f, 0.0f, 1.0f, Float.POSITIVE_INFINITY));
        assertFalse(SkateCameraMath.hasFrustumOffsetProgress(1.0f, 0.0f, 0.0f, 0.0f));
        assertFalse(SkateCameraMath.hasFrustumOffsetProgress(1.0f, 0.0f, 0.01f, 0.0f));
        assertTrue(SkateCameraMath.isFinite(new Vec3(1.0, 64.0, -2.0)));
        assertTrue(SkateCameraMath.isFinite(new SkateCameraMath.Rotation(45.0f, -10.0f)));
        assertTrue(SkateCameraMath.isFiniteViewVector(0.0f, 0.0f, 1.0f, 0.0f));
        assertTrue(SkateCameraMath.hasFrustumOffsetProgress(0.9f, 0.0f, 0.1f, 0.0f));
    }
}
