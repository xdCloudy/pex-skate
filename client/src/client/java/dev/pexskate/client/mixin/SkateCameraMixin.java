package dev.pexskate.client.mixin;

import dev.pexskate.client.SkatePoseCache;
import net.minecraft.client.Camera;
import net.minecraft.client.Minecraft;
import net.minecraft.client.DeltaTracker;
import net.minecraft.client.renderer.culling.Frustum;
import net.minecraft.client.renderer.state.level.CameraRenderState;
import net.minecraft.world.level.ClipContext;
import net.minecraft.world.phys.Vec3;
import org.joml.Matrix3f;
import org.joml.Quaternionf;
import org.joml.Vector3f;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.gen.Invoker;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/** Apply the Skate engine's chase-camera pose to Minecraft's render camera. */
@Mixin(Camera.class)
public abstract class SkateCameraMixin {
    @Shadow private float depthFar;
    @Shadow protected abstract void setPosition(Vec3 position);
    @Shadow protected abstract void setRotation(float yaw, float pitch);
    @Invoker("setupPerspective")
    protected abstract void pexskate$setupPerspective(float near, float far, float fov, float width, float height);

    @Inject(method = "extractRenderState(Lnet/minecraft/client/renderer/state/level/CameraRenderState;Lnet/minecraft/client/DeltaTracker;)V", at = @At("HEAD"))
    private void pexskate$applyEngineCamera(CameraRenderState state, DeltaTracker delta, CallbackInfo ci) {
        Minecraft client = Minecraft.getInstance();
        if (client.player == null || client.level == null) return;
        SkatePoseCache.Pose pose = SkatePoseCache.sample(client.player.getId(), true);
        if (pose == null || !pose.hasCamera()) return;

        float[] camera = pose.cameraPosition();
        float[] forward = pose.cameraForward();
        if (camera == null) return;
        Vec3 target = new Vec3(camera[0], camera[1], camera[2]);
        float[] hipsPoint = pose.point(1);
        if (hipsPoint == null) return;
        Vec3 hips = new Vec3(hipsPoint[0], hipsPoint[1], hipsPoint[2]);
        Vec3 direction = target.subtract(hips).normalize();
        if (direction.lengthSqr() > 1.0e-6) {
            var hit = client.level.clip(new ClipContext(
                    hips, target, ClipContext.Block.COLLIDER, ClipContext.Fluid.NONE, client.player));
            if (hit.getType() == net.minecraft.world.phys.HitResult.Type.BLOCK) {
                target = hit.getLocation().subtract(direction.scale(0.15));
            }
        }

        float yaw = (float) Math.toDegrees(Math.atan2(-forward[0], forward[2]));
        float pitch = (float) -Math.toDegrees(Math.asin(Math.clamp(forward[1], -1.0f, 1.0f)));
        this.setPosition(target);
        this.setRotation(yaw, pitch);

        float fov = pose.cameraFov();
        if (Float.isFinite(fov) && fov >= 30.0f && fov <= 120.0f) {
            this.pexskate$setupPerspective(0.05f, depthFar, fov,
                    client.getWindow().getWidth(), client.getWindow().getHeight());
        }
    }

    @Inject(method = "extractRenderState(Lnet/minecraft/client/renderer/state/level/CameraRenderState;Lnet/minecraft/client/DeltaTracker;)V", at = @At("TAIL"))
    private void pexskate$refreshCameraFrustum(CameraRenderState state, DeltaTracker delta, CallbackInfo ci) {
        Minecraft client = Minecraft.getInstance();
        if (client.player == null) return;
        SkatePoseCache.Pose pose = SkatePoseCache.sample(client.player.getId(), true);
        if (pose == null || !pose.hasCamera()) return;

        float[] forwardValues = pose.cameraForward();
        float[] upValues = pose.cameraUp();
        Vector3f forward = new Vector3f(forwardValues[0], forwardValues[1], forwardValues[2]).normalize();
        Vector3f up = new Vector3f(upValues[0], upValues[1], upValues[2]).normalize();
        Vector3f right = up.cross(forward, new Vector3f()).normalize();
        up = forward.cross(right, new Vector3f()).normalize();
        Quaternionf orientation = new Quaternionf().setFromUnnormalized(new Matrix3f()
                .setColumn(0, right)
                .setColumn(1, up)
                .setColumn(2, new Vector3f(forward).negate()));
        state.orientation.set(orientation);
        state.xRot = (float) -Math.toDegrees(Math.asin(Math.clamp(forward.y, -1.0f, 1.0f)));
        state.yRot = (float) Math.toDegrees(Math.atan2(-forward.x, forward.z));
        state.viewRotationMatrix.identity().rotation(new Quaternionf(orientation).conjugate());

        Frustum frustum = new Frustum(state.projectionMatrix, state.viewRotationMatrix);
        frustum.prepare(state.pos.x, state.pos.y, state.pos.z);
        state.cullFrustum = frustum;
    }
}
