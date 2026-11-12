package dep;
public class Methods {
    public int field;
    public static Methods SHARED;
    public void finish(int x) {}
    public int zero() { return 0; }
    public int pick(int x) { return x; }
    public long pick(long x) { return x; }
    public Token token(Token x) { return x; }
    public int[] arrayResult(int x) { return null; }
    public final int finalMethod(int x) { return x; }
    public synchronized int syncMethod(int x) { return x; }
    public native int nativeMethod(int x);
    public static int staticMethod(int x) { return x; }
    public int vararg(int... x) { return 0; }
    public <T> T generic(T x) { return x; }
    public int arrayFormal(int[] x) { return 0; }
    private int secret(int x) { return x; }
    protected int protectedMethod(int x) { return x; }
}
