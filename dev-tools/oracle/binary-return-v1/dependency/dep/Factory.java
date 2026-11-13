package dep;
public class Factory {
  public boolean booleanValue() { return false; }
  public byte byteValue() { return 0; }
  public char charValue() { return 0; }
  public short shortValue() { return 0; }
  public int intValue() { return 0; }
  public long longValue() { return 0L; }
  public float floatValue() { return 0.0f; }
  public double doubleValue() { return 0.0d; }
  public Product value() { return null; }
  public Product choose(long x) { return null; }
  public Other choose(int x) { return null; }
  public Product[] array() { return null; }
  public Nested.Item nested() { return null; }
  public Box<String> box() { return null; }
  public static Product staticValue() { return null; }
  public Product bad(int x) { return null; }
  public <T> T bad(T x) { return x; }
  public void voidValue() {}
}
