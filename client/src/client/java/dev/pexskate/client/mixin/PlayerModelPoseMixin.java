package dev.pexskate.client.mixin;

import dev.pexskate.client.SkatePoseCache;
import net.minecraft.client.Minecraft;
import net.minecraft.client.model.player.PlayerModel;
import net.minecraft.client.renderer.entity.state.AvatarRenderState;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/** Retarget the Skate 3 skeleton points onto Minecraft's six-part player rig. */
@Mixin(PlayerModel.class)
public abstract class PlayerModelPoseMixin {
    @Inject(method = "setupAnim(Lnet/minecraft/client/renderer/entity/state/AvatarRenderState;)V", at = @At("TAIL"))
    private void pexskate$applySkatePose(AvatarRenderState state, CallbackInfo ci) {
        Minecraft client = Minecraft.getInstance();
        boolean localEntity = client.player != null && client.player.getId() == state.id;
        SkatePoseCache.Pose pose = SkatePoseCache.sample(state.id, localEntity);
        if (pose == null) return;

        PlayerModel model = (PlayerModel) (Object) this;

        float[] hips = pose.point(1);
        float[] spine = pose.point(5);
        float[] leftHip = pose.point(21);
        float[] rightHip = pose.point(17);
        if (hips == null || spine == null || leftHip == null || rightHip == null) return;

        float[] up = normalize(subtract(spine, hips));
        float[] right = normalize(subtract(rightHip, leftHip));
        float[] forward = normalize(cross(up, right));
        if (length(up) < 0.5f || length(right) < 0.5f || length(forward) < 0.5f) return;

        // The chase camera follows the torso heading derived from this same
        // skeletal pose. Board yaw can spin independently during tricks, so
        // using it for the fallback avatar turns the visible skater away from
        // the camera's follow direction.
        model.body.yRot = pose.characterYaw();
        setDownDirection(model.body, subtract(hips, spine), up, right, forward);

        float[] head = pose.point(8);
        float[] neck = pose.point(6);
        if (head != null && neck != null) setDownDirection(model.head, subtract(neck, head), up, right, forward);
        model.hat.xRot = model.head.xRot;
        model.hat.yRot = model.head.yRot;
        model.hat.zRot = model.head.zRot;

        applyLimb(model.rightArm, pose.point(10), pose.point(11), up, right, forward);
        applyLimb(model.leftArm, pose.point(14), pose.point(15), up, right, forward);
        applyLimb(model.rightLeg, pose.point(17), pose.point(18), up, right, forward);
        applyLimb(model.leftLeg, pose.point(21), pose.point(22), up, right, forward);

        copyRotation(model.rightSleeve, model.rightArm);
        copyRotation(model.leftSleeve, model.leftArm);
        copyRotation(model.jacket, model.body);
        copyRotation(model.rightPants, model.rightLeg);
        copyRotation(model.leftPants, model.leftLeg);
    }

    private static void applyLimb(net.minecraft.client.model.geom.ModelPart part,
                                  float[] start, float[] end,
                                  float[] up, float[] right, float[] forward) {
        if (start != null && end != null) setDownDirection(part, subtract(end, start), up, right, forward);
    }

    /** A neutral humanoid segment points down the model's local +Y axis. */
    private static void setDownDirection(net.minecraft.client.model.geom.ModelPart part,
                                         float[] worldDirection,
                                         float[] up, float[] right, float[] forward) {
        float[] direction = normalize(worldDirection);
        float x = dot(direction, right);
        float y = -dot(direction, up);
        float z = dot(direction, forward);
        part.xRot = (float) Math.atan2(z, Math.sqrt(x * x + y * y));
        part.zRot = (float) Math.atan2(-x, y);
        part.yRot = 0.0f;
    }

    private static void copyRotation(net.minecraft.client.model.geom.ModelPart target,
                                     net.minecraft.client.model.geom.ModelPart source) {
        target.xRot = source.xRot;
        target.yRot = source.yRot;
        target.zRot = source.zRot;
    }

    private static float[] subtract(float[] a, float[] b) {
        return new float[] {a[0] - b[0], a[1] - b[1], a[2] - b[2]};
    }

    private static float[] cross(float[] a, float[] b) {
        return new float[] {a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]};
    }

    private static float dot(float[] a, float[] b) { return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]; }
    private static float length(float[] a) { return (float) Math.sqrt(dot(a, a)); }
    private static float[] normalize(float[] a) {
        float length = length(a);
        return length > 1.0e-5f ? new float[] {a[0] / length, a[1] / length, a[2] / length} : new float[3];
    }
}
