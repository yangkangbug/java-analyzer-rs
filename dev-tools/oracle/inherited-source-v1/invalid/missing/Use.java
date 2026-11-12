package broken;
class Child extends Missing { public void /*D:child*/pick(int x) {} }
class Use { void run(Child c) { c./*Q:missing_parent*/pick(1); } }
