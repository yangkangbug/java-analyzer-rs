package inventory;
class Token {}
class Box<T> { public int /*D:target*/take(Token value) { return 1; } }
class Factory { public Box<?> make() { return null; } }
class Use { public int run(Factory factory, Token token) { return factory.make()./*Q:probe*/take(token); } }
