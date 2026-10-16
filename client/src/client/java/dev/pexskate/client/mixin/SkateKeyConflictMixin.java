package dev.pexskate.client.mixin;

import dev.pexskate.client.PexSkateClient;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.Minecraft;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/** Keep SkateGM's Q/E grab keys from also dropping an item or opening inventory. */
@Mixin(KeyMapping.class)
public abstract class SkateKeyConflictMixin {
    @Shadow private int clickCount;

    @Inject(method = "consumeClick", at = @At("HEAD"), cancellable = true)
    private void pexskate$suppressVanillaGrabConflicts(CallbackInfoReturnable<Boolean> cir) {
        if (!PexSkateClient.isSkateModeActive()) return;

        Minecraft client = Minecraft.getInstance();
        KeyMapping mapping = (KeyMapping) (Object) this;
        if (mapping == client.options.keyDrop || mapping == client.options.keyInventory) {
            clickCount = 0;
            cir.setReturnValue(false);
        }
    }
}
