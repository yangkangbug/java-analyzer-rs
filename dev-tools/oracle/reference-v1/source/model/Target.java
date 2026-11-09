// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package model;
public class Target {
    public void /*D:pick_root*/pick(Root value) {}
    public void /*D:pick_mid*/pick(Mid value) {}
    public void /*D:pick_other*/pick(Other value) {}
    public void /*D:exact_leaf*/exact(Leaf value) {}
    public void /*D:exact_other*/exact(Other value) {}
    public void /*D:only_root*/only(Root value) {}
    public void /*D:empty*/empty() {}
    public void /*D:arity_one*/arity(Root value) {}
    public void /*D:arity_two*/arity(Root first, Root second) {}
    public void /*D:binary_object*/binary(Object value) {}
    public void /*D:binary_other*/binary(Other value) {}
    public void /*D:marker_interface*/marker(Marker value) {}
    public void /*D:marker_other*/marker(Other value) {}
    public void /*D:cross_root*/cross(Root first, Root second) {}
    public void /*D:cross_mid*/cross(Mid first, Root second) {}
}
