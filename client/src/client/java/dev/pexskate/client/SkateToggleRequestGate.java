package dev.pexskate.client;

/** Limits toggle requests while the server is loading or leaving a Skate session. */
final class SkateToggleRequestGate {
    private static final long RETRY_AFTER_NANOS = 30_000_000_000L;

    private boolean pending;
    private long pendingSinceNanos;

    boolean begin(long nowNanos) {
        if (pending && nowNanos - pendingSinceNanos < RETRY_AFTER_NANOS) {
            return false;
        }
        pending = true;
        pendingSinceNanos = nowNanos;
        return true;
    }

    void acknowledge() {
        pending = false;
    }

    void reset() {
        acknowledge();
    }
}
