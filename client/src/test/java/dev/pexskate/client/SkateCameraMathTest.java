package dev.pexskate.client;

import net.minecraft.world.phys.Vec3;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;

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
}
