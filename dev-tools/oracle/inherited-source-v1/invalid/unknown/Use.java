package broken;
class Parent { public void /*D:parent*/pick(Missing x) {} }
class Child extends Parent { public void /*D:child*/pick(Missing x) {} }
class Use { void run(Child c, Missing x) { c./*Q:unknown_signature*/pick(x); } }
