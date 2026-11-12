package core;
// Unicode 🧭 coordinate sentinel
class Parent {
    public void /*D:unique*/unique(int x) {}
    public void /*D:parent_pick*/pick(int x) {}
    public void /*D:parent_override*/over(int x) {}
}
class Child extends Parent {
    public void /*D:child_pick*/pick(long x) {}
}
class Grandchild extends Child {}
class Override extends Parent {
    public void /*D:child_override*/over(int x) {}
}
interface Face { default void /*D:face*/face(int x) {} }
class Impl implements Face {}
class Generic<T> { public void /*D:generic*/generic(int x) {} }
class Bound extends Generic<String> {}
class Use {
    void run(Child c, Grandchild g, Override o, Impl i, Bound b, int n, long l) {
        c./*Q:unique_parent*/unique(n);
        c./*Q:parent_overload*/pick(n);
        c./*Q:child_overload*/pick(l);
        g./*Q:two_level*/unique(n);
        o./*Q:override_refusal*/over(n);
        i./*Q:interface_refusal*/face(n);
        b./*Q:generic_refusal*/generic(n);
    }
}
