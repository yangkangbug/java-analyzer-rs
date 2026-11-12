package bad;
class Parent { public void /*D:parent*/call(Missing x) {} }
class Child extends Parent { public void /*D:child*/call(Missing x) {} }
class Use { void run(Child c, Missing x) { c./*Q:invalid_unknown*/call(x); } }
