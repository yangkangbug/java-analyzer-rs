package casts;
public class Target {
    public void /*D:parent*/pick(Parent value) {}
    public void /*D:child*/pick(Child value) {}
    public void /*D:array*/array(Parent[] value) {}
    public void /*D:generic*/generic(Box<String> value) {}
    public void /*D:string*/text(String value) {}
    public void /*D:comparable*/text(Number value) {}
    public Parent make() { return null; }
}
