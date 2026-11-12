package bad;
class Parent { public final void /*D:parent*/call(int x) {} }
class Child extends Parent { public void /*D:child*/call(int x) {} }
class Use { void run(Child c) { c./*Q:invalid_final*/call(1); } }
