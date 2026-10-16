package dev.pexskate.client.mixin;

import dev.pexskate.client.SkateMapModel;
import net.minecraft.client.Camera;
import net.minecraft.client.DeltaTracker;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.client.renderer.fog.FogData;
import net.minecraft.client.renderer.fog.FogRenderer;
import net.minecraft.world.level.material.FogType;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/** Applies the converted district fog while the local player is skating. */
@Mixin(FogRenderer.class)
public abstract class SkateDistrictFogMixin {
    @Inject(method = "setupFog(Lnet/minecraft/client/Camera;ILnet/minecraft/client/DeltaTracker;FLnet/minecraft/client/multiplayer/ClientLevel;)Lnet/minecraft/client/renderer/fog/FogData;",
            at = @At("RETURN"))
    private void pexskate$applyDistrictFog(Camera camera, int renderDistance, DeltaTracker delta,
                                           float partialTick, ClientLevel level,
                                           CallbackInfoReturnable<FogData> callback) {
        if (camera.getFluidInCamera() != FogType.NONE) return;
        SkateMapModel.FogSettings settings = SkateMapModel.localFogSettings();
        if (settings == null || settings.maximum() <= 0.0f) return;

        FogData fog = callback.getReturnValue();
        float power = Math.clamp(settings.power(), 0.125f, 8.0f);
        float range = settings.far() - settings.near();
        float powerAdjustedRange = (float) (range * Math.pow(2.0, 1.0 - 1.0 / power));
        fog.environmentalStart = Math.max(0.0f, settings.near());
        fog.environmentalEnd = Math.max(fog.environmentalStart + 0.01f,
                fog.environmentalStart + powerAdjustedRange);

        float amount = Math.clamp(settings.maximum(), 0.0f, 1.0f);
        fog.color.set(
                fog.color.x() * (1.0f - amount) + settings.red() * amount,
                fog.color.y() * (1.0f - amount) + settings.green() * amount,
                fog.color.z() * (1.0f - amount) + settings.blue() * amount,
                fog.color.w());
    }
}
