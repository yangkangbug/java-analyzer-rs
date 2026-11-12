package api;
import model.Product;
public class Factory {
    public Product make() { return new model.Child(); }
    public Product identity(Product value) { return value; }
    public int number() { return 1; }
    public int integer(int value) { return value; }
    public String text() { return "text"; }
    public <T extends Product> T generic() { return null; }
    public Product[] array() { return null; }
    public void nothing() {}
}
