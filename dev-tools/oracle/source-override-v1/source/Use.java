package overrides;
// UTF-8 🧭 scope marker
class Product {}
class SubProduct extends Product {}
class Parent {
    public void /*D:parent_pick*/pick(int x) {}
    public void /*D:parent_wide*/pick(long x) {}
    public Product /*D:parent_make*/make() { return null; }
    public Product /*D:parent_same*/same() { return null; }
}
class Child extends Parent {
    @Override public void /*D:child_pick*/pick(int x) {}
    @Override public SubProduct /*D:child_make*/make() { return null; }
    public Product /*D:child_same*/same() { return null; }
}
class Grandchild extends Child {
    @Override public void /*D:grand_pick*/pick(int x) {}
}
class FinalChild extends Parent {
    public final void /*D:final_child*/pick(int x) {}
}
class Use {
    void run(Parent parent, Child child, Grandchild grand, FinalChild fin, int i, long l) {
        child./*Q:child_override*/pick(i);
        grand./*Q:multilevel_override*/pick(i);
        grand./*Q:inherited_overload*/pick(l);
        parent./*Q:parent_static_receiver*/pick(i);
        child./*Q:covariant_return*/make();
        child./*Q:identical_return*/same();
        fin./*Q:final_leaf*/pick(i);
    }
}
