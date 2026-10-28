import io.github.thebusybiscuit.slimefun4.core.services.nativeengine.RustNativeEngine;
import java.lang.management.ManagementFactory;
import java.util.Random;

/** Benchmark Java vs Rust (JNI). Uso: bench/run.sh */
public class Bench {
    static final com.sun.management.ThreadMXBean TM =
        (com.sun.management.ThreadMXBean) ManagementFactory.getThreadMXBean();
    static volatile long sink;

    // --- referencias Java (identicas a RustNativeEngine/EnergyNet) ---
    static int sumOld(int[] values, int n) { // patron anterior: new int[] + copyOf + suma
        int[] c = java.util.Arrays.copyOf(values, n);
        int r = 0;
        for (int v : c) { long x = (long) r + v; r = x > Integer.MAX_VALUE ? Integer.MAX_VALUE : x < Integer.MIN_VALUE ? Integer.MIN_VALUE : (int) x; }
        return r;
    }
    static int addSat(int a, int b) { long x = (long) a + b; return x > Integer.MAX_VALUE ? Integer.MAX_VALUE : x < Integer.MIN_VALUE ? Integer.MIN_VALUE : (int) x; }
    static int sumNew(int[] values, int n) { int r = 0; for (int i = 0; i < n; i++) r = addSat(r, values[i]); return r; }

    static int fillJava(int supply, int[] cap, int[] ch, int[] out2) {
        int remaining = supply, demand = 0;
        for (int i = 0; i < cap.length; i++) {
            int c = ch[i];
            if (c < cap[i]) {
                int space = cap[i] - c;
                demand = addSat(demand, space);
                if (remaining > 0) {
                    if (remaining > space) { ch[i] = cap[i]; remaining -= space; }
                    else { ch[i] = c + remaining; remaining = 0; }
                }
            }
        }
        out2[0] = remaining; out2[1] = demand; return 0;
    }

    interface Op { void run(); }

    /** ns/op y bytes/op asignados, tras calentar. */
    static double[] measure(Op op, int iters) {
        for (int i = 0; i < iters; i++) op.run();       // warmup (JIT C2)
        for (int i = 0; i < iters; i++) op.run();
        long t = Thread.currentThread().getId();
        long a0 = TM.getThreadAllocatedBytes(t), n0 = System.nanoTime();
        for (int i = 0; i < iters; i++) op.run();
        long n1 = System.nanoTime(), a1 = TM.getThreadAllocatedBytes(t);
        return new double[] {(double) (n1 - n0) / iters, (double) (a1 - a0) / iters};
    }

    static void row(String name, int size, double[] r) {
        System.out.printf("%-34s n=%-6d %10.1f ns/op %9.1f B/op%n", name, size, r[0], r[1]);
    }

    public static void main(String[] args) {
        System.load(args[0]);
        System.out.println("ABI=" + RustNativeEngine.nativeAbiVersion() + " java=" + System.getProperty("java.version"));
        Random rnd = new Random(42);
        for (int n : new int[] {2, 16, 128, 1024, 8192, 65536}) {
            int iters = Math.max(2000, 4_000_000 / n);
            int[] vals = new int[n];
            for (int i = 0; i < n; i++) vals[i] = rnd.nextInt(1000);
            if (sumNew(vals, n) != RustNativeEngine.nativeSumSaturating(vals) || sumOld(vals, n) != sumNew(vals, n))
                throw new AssertionError("sum difiere");
            row("sum Java viejo (copyOf)", n, measure(() -> sink = sumOld(vals, n), iters));
            row("sum Java sin alloc", n, measure(() -> sink = sumNew(vals, n), iters));
            row("sum Rust JNI critical", n, measure(() -> sink = RustNativeEngine.nativeSumSaturating(vals), iters));

            int[] cap = new int[n], base = new int[n], a = new int[n], b = new int[n];
            int[] oa = new int[2], ob = new int[2];
            for (int i = 0; i < n; i++) { cap[i] = 100 + rnd.nextInt(10000); base[i] = rnd.nextInt(cap[i] + 1); }
            // paridad exacta con varios supplies, incluido caso de saturacion
            for (int supply : new int[] {0, 1, 5000, n * 3000, Integer.MAX_VALUE}) {
                System.arraycopy(base, 0, a, 0, n); System.arraycopy(base, 0, b, 0, n);
                fillJava(supply, cap, a, oa);
                if (RustNativeEngine.nativeFillSequential(supply, cap, b, ob) != 0 || !java.util.Arrays.equals(a, b) || !java.util.Arrays.equals(oa, ob))
                    throw new AssertionError("fill difiere supply=" + supply);
            }
            // el reset de carga se incluye en ambos para ser justos (copia identica)
            int supply = n * 1500;
            row("fill Java (+reset)", n, measure(() -> { System.arraycopy(base, 0, a, 0, n); sink = fillJava(supply, cap, a, oa); }, iters));
            row("fill Rust JNI (+reset)", n, measure(() -> { System.arraycopy(base, 0, b, 0, n); sink = RustNativeEngine.nativeFillSequential(supply, cap, b, ob); }, iters));
            System.out.println();
        }
    }
}
