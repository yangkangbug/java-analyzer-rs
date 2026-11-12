package bad;

class Child extends Missing { public void /*D:child*/call(int x) {} }
class Use { void run(Child c) { c./*Q:invalid_missing*/call(1); } }
