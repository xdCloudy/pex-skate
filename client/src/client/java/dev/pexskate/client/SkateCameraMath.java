package dev.pexskate.client;

import net.minecraft.world.phys.Vec3;

/** Camera geometry shared by the pose-anchored Minecraft-world chase camera. */
public final class SkateCameraMath {
    private SkateCameraMath() {}

    public static Vec3 minecraftChasePosition(Vec3 root, float yaw) {
        Vec3 forward = new Vec3(-Math.sin(yaw), 0.0, Math.cos(yaw));
        return root.subtract(forward.scale(4.0)).add(0.0, 1.55, 0.0);
    }

    public static Vec3 minecraftChaseLookAt(Vec3 root) {
        return root.add(0.0, 0.75, 0.0);
    }

    public static Rotation lookAt(Vec3 camera, Vec3 target) {
        Vec3 view = target.subtract(camera).normalize();
        float yaw = (float) Math.toDegrees(Math.atan2(-view.x, view.z));
        float pitch = (float) -Math.toDegrees(Math.asin(Math.clamp(view.y, -1.0, 1.0)));
        return new Rotation(yaw, pitch);
    }

    public record Rotation(float yaw, float pitch) {}
}
