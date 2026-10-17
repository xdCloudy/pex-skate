package dev.pexskate.client.mixin;

import dev.pexskate.client.PexSkateClient;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.Minecraft;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/** Keep active Skate bindings from also triggering their Minecraft actions. */
@Mixin(KeyMapping.class)
public abstract class SkateKeyConflictMixin {
    @Shadow private int clickCount;

    @Inject(method = "isDown", at = @At("HEAD"), cancellable = true)
    private void pexskate$suppressVanillaHeldConflicts(CallbackInfoReturnable<Boolean> cir) {
        Minecraft client = Minecraft.getInstance();
        if (PexSkateClient.shouldSuppressVanillaKey((KeyMapping) (Object) this, client)) {
            cir.setReturnValue(false);
        }
    }

    @Inject(method = "consumeClick", at = @At("HEAD"), cancellable = true)
    private void pexskate$suppressVanillaGrabConflicts(CallbackInfoReturnable<Boolean> cir) {
        Minecraft client = Minecraft.getInstance();
        KeyMapping mapping = (KeyMapping) (Object) this;
        if (PexSkateClient.shouldSuppressVanillaKey(mapping, client)) {
            clickCount = 0;
            cir.setReturnValue(false);
        }
    }
}
