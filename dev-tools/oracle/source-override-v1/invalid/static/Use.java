package bad;
class Parent { public void /*D:parent*/call(int x) {} }
class Child extends Parent { public static void /*D:child*/call(int x) {} }
class Use { void run(Child c) { c./*Q:invalid_static*/call(1); } }
