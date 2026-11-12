package invalid;
class Product {}
class Factory { public void nothing() {} }
class Sink { public void /*D:accept*/accept(Product value) {} }
class Use { void run(Factory f, Sink s) { s./*Q:invalid_unknown*/accept(absent()); } }
