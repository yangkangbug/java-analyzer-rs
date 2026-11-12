package bad;
class Parent { public int /*D:parent*/call(int x) { return 0; } }
class Child extends Parent { public long /*D:child*/call(int x) { return 0; } }
class Use { void run(Child c) { c./*Q:invalid_return*/call(1); } }
