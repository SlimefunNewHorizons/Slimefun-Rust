package io.github.thebusybiscuit.slimefun4.core.services.nativeengine;

/**
 * Stub de benchmark: mismo FQCN que la clase real de Slimefun4-Drake para que los simbolos JNI de
 * libslimefun_ffi.so (Java_io_github_..._RustNativeEngine_native*) enlacen sin Bukkit.
 */
public final class RustNativeEngine {
    private RustNativeEngine() {}

    public static native int nativeAbiVersion();
    public static native int nativeSumSaturating(int[] values);
    public static native int nativeFillSequential(int supply, int[] capacity, int[] charge, int[] out2);
    public static native int nativeStoreRemaining(int remaining, int[] capacity, int[] out);
}
