package dev.pexskate.client.mixin;

import dev.pexskate.client.SkatePoseCache;
import dev.pexskate.client.SkateMapModel;
import dev.pexskate.client.SkateCameraMath;
import net.minecraft.client.Camera;
import net.minecraft.client.Minecraft;
import net.minecraft.client.DeltaTracker;
import net.minecraft.client.renderer.Projection;
import net.minecraft.client.renderer.state.level.CameraRenderState;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.ClipContext;
import net.minecraft.world.phys.Vec3;
import org.joml.Matrix3f;
import org.joml.Matrix4f;
import org.joml.Matrix4fc;
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
    @Shadow private Projection projection;
    @Shadow protected abstract void setPosition(Vec3 position);
    @Shadow protected abstract void setRotation(float yaw, float pitch);
    @Shadow public abstract Vec3 position();
    @Shadow public abstract BlockPos blockPosition();
    @Shadow public abstract Quaternionf rotation();
    @Invoker("setupPerspective")
    protected abstract void pexskate$setupPerspective(float near, float far, float fov, float width, float height);
    @Invoker("prepareCullFrustum")
    protected abstract void pexskate$prepareCullFrustum(Matrix4fc projection, Matrix4f view, Vec3 position);
    @Invoker("getCullFrustum")
    protected abstract net.minecraft.client.renderer.culling.Frustum pexskate$getCullFrustum();

    @Inject(method = "extractRenderState(Lnet/minecraft/client/renderer/state/level/CameraRenderState;Lnet/minecraft/client/DeltaTracker;)V", at = @At("TAIL"))
    private void pexskate$applyEngineCamera(CameraRenderState state, DeltaTracker delta, CallbackInfo ci) {
        Minecraft client = Minecraft.getInstance();
        if (client.player == null || client.level == null) return;
        SkatePoseCache.Pose pose = SkatePoseCache.sampleForRenderCamera(client.player.getId());
        if (pose == null) return;

        if (!SkateMapModel.hasActiveRetailMap(client.player.getId())) {
            // Follow the exact root used by LivingEntityRenderer for this
            // frame. Hip joints bob and shift during pushes and tricks, so
            // anchoring to them makes the camera drift against the rendered
            // player even though both are sampled from the same pose.
            float[] root = pose.rootPosition();
            Vec3 anchor = new Vec3(root[0], root[1], root[2]);
            Vec3 lookAt = SkateCameraMath.minecraftChaseLookAt(anchor);
            // Board yaw can spin independently while the skater performs a
            // trick. Use the torso heading so the chase camera stays aligned
            // with the same character pose rendered this frame.
            Vec3 target = SkateCameraMath.minecraftChasePosition(anchor, pose.characterYaw());

            double segmentLength = lookAt.distanceTo(target);
            if (segmentLength > 1.0e-6) {
                var hit = client.level.clip(new ClipContext(
                        lookAt, target, ClipContext.Block.COLLIDER, ClipContext.Fluid.NONE, client.player));
                if (hit.getType() == net.minecraft.world.phys.HitResult.Type.BLOCK) {
                    double safeFraction = Math.max(0.0,
                            lookAt.distanceTo(hit.getLocation()) / segmentLength - 0.15 / segmentLength);
                    target = lookAt.lerp(target, safeFraction);
                }
            }

            SkateCameraMath.Rotation rotation = SkateCameraMath.lookAt(target, lookAt);
            this.setPosition(target);
            this.setRotation(rotation.yaw(), rotation.pitch());
            pexskate$syncRenderState(state, this.rotation(), rotation.yaw(), rotation.pitch());
            return;
        }

        if (!pose.hasCamera()) return;

        float[] camera = pose.cameraPosition();
        float[] forward = pose.cameraForward();
        if (camera == null) return;
        Vec3 target = new Vec3(camera[0], camera[1], camera[2]);
        float[] hipsPoint = pose.point(1);
        if (hipsPoint == null) return;
        Vec3 hips = new Vec3(hipsPoint[0], hipsPoint[1], hipsPoint[2]);
        Vec3 direction = target.subtract(hips).normalize();
        double segmentLength = target.distanceTo(hips);
        if (direction.lengthSqr() > 1.0e-6 && segmentLength > 1.0e-6) {
            double closestFraction = SkateMapModel.clipCameraFraction(hips, target);
            var hit = client.level.clip(new ClipContext(
                    hips, target, ClipContext.Block.COLLIDER, ClipContext.Fluid.NONE, client.player));
            if (hit.getType() == net.minecraft.world.phys.HitResult.Type.BLOCK) {
                closestFraction = Math.min(closestFraction, hips.distanceTo(hit.getLocation()) / segmentLength);
            }
            if (closestFraction < 1.0) {
                double safeFraction = Math.max(0.0, closestFraction - 0.15 / segmentLength);
                target = hips.lerp(target, safeFraction);
            }
        }

        float yaw = (float) Math.toDegrees(Math.atan2(-forward[0], forward[2]));
        float pitch = (float) -Math.toDegrees(Math.asin(Math.clamp(forward[1], -1.0f, 1.0f)));
        this.setPosition(target);
        this.setRotation(yaw, pitch);

        float[] upValues = pose.cameraUp();
        Vector3f cameraForward = new Vector3f(forward[0], forward[1], forward[2]).normalize();
        Vector3f up = new Vector3f(upValues[0], upValues[1], upValues[2]).normalize();
        Vector3f right = up.cross(cameraForward, new Vector3f()).normalize();
        up = cameraForward.cross(right, new Vector3f()).normalize();
        Quaternionf orientation = new Quaternionf().setFromUnnormalized(new Matrix3f()
                .setColumn(0, right)
                .setColumn(1, up)
                .setColumn(2, new Vector3f(cameraForward).negate()));
        pexskate$syncRenderState(state, orientation, yaw, pitch);

        float fov = pose.cameraFov();
        if (Float.isFinite(fov) && fov >= 30.0f && fov <= 120.0f) {
            this.pexskate$setupPerspective(0.05f, depthFar, fov,
                    client.getWindow().getWidth(), client.getWindow().getHeight());
            projection.getMatrix(state.projectionMatrix);
        }
    }

    private void pexskate$syncRenderState(CameraRenderState state, Quaternionf orientation, float yaw, float pitch) {
        state.pos = this.position();
        state.blockPos = this.blockPosition();
        state.xRot = pitch;
        state.yRot = yaw;
        state.orientation.set(orientation);
        state.viewRotationMatrix.identity().rotation(new Quaternionf(orientation).conjugate());
        pexskate$prepareCullFrustum(state.projectionMatrix, state.viewRotationMatrix, state.pos);
        state.cullFrustum.set(pexskate$getCullFrustum());
    }
}
