package dev.pexskate.client;

/** Pure policy for avoiding collisions between active Skate and Minecraft bindings. */
final class SkateKeyConflictPolicy {
    private SkateKeyConflictPolicy() {}

    static boolean shouldSuppress(boolean skateMode, boolean bindingsOverlap, boolean actionDown) {
        return skateMode && bindingsOverlap && actionDown;
    }
}
