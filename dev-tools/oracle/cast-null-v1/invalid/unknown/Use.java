package casts;
class Parent {}
class Target {
    public void /*D:only*/pick(Parent value) {}
}
class Use { void run(Target t) { t./*Q:invalid_unknown*/pick((Missing) null); } }
