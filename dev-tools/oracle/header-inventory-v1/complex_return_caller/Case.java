package inventory;
class Token {}
class Box<T> {}
class Outer<T> { class Inner<U> {} }
class Sink {
    public int /*D:narrow*/take(Token value) { return 1; }
    public int /*D:wide*/take(Object value) { return 2; }
}
class Use {
    public Box<?> ignored() { return null; }
    public Box<?> run(Sink sink, Token token) { sink./*Q:probe*/take(token); return null; }
}
