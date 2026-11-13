package dep;
public class Sink {
  public boolean pick(boolean x) { return x; }
  public byte pick(byte x) { return x; }
  public char pick(char x) { return x; }
  public short pick(short x) { return x; }
  public int pick(int x) { return x; }
  public long pick(long x) { return x; }
  public float pick(float x) { return x; }
  public double pick(double x) { return x; }
  public int accept(Product x) { return 1; }
  public int accept(Object x) { return 2; }
  public int takeObject(Object x) { return 3; }
}
