package casts;
class Parent {}
class Target {
    public void /*D:only*/pick(int value) {}
}
class Use { void run(Target t) { t./*Q:invalid_primitive*/pick((int) null); } }
