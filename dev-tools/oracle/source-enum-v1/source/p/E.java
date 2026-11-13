package p;
public enum E {
    /*D:a*/A(E./*Q:argument*/CODE) {
        int /*D:hidden*/hidden;
        int read() { return /*Q:body_outer*/CODE + /*Q:body_hidden*/hidden; }
    },
    /*D:b*/B(2),
    /*D:han*/雪(3);
    public static final int /*D:code*/CODE = 1;
    public int /*D:ordinary*/ordinary;
    E(int n) {}
    public E pick() { return /*Q:unqualified*/A; }
}
