package dev.pexskate.client.mixin;

import dev.pexskate.client.SkateCameraMath;
import net.minecraft.client.renderer.culling.Frustum;
import org.joml.Vector4f;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/** Avoid Minecraft's unbounded camera-cube offset loop when its view vector is invalid. */
@Mixin(Frustum.class)
public abstract class SkateFrustumSafetyMixin {
    private static final Logger LOGGER = LoggerFactory.getLogger("pexskate/camera");
    private static boolean invalidViewVectorReported;
    private static boolean frustumVectorObserved;

    @Shadow private Vector4f viewVector;

    @Inject(method = "offsetToFullyIncludeCameraCube", at = @At("HEAD"), cancellable = true)
    private void pexskate$skipOffsetForInvalidViewVector(
            int cubeSize, CallbackInfoReturnable<Frustum> callback) {
        if (!frustumVectorObserved) {
            frustumVectorObserved = true;
            if (viewVector == null) LOGGER.info("Minecraft frustum view vector was null on first cull pass");
            else LOGGER.info("Minecraft frustum view vector on first cull pass: [{}, {}, {}, {}]",
                    viewVector.x(), viewVector.y(), viewVector.z(), viewVector.w());
        }
        if (viewVector != null && SkateCameraMath.hasFrustumOffsetProgress(
                viewVector.x(), viewVector.y(), viewVector.z(), viewVector.w())) return;
        if (!invalidViewVectorReported) {
            invalidViewVectorReported = true;
            LOGGER.warn("Skipped camera frustum offset because its view vector was non-finite or could not advance along Z: {}",
                    viewVector);
        }
        callback.setReturnValue((Frustum) (Object) this);
    }
}
