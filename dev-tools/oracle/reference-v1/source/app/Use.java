// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package app;
import model.Target;
import model.Root;
import model.Mid;
import model.Leaf;
import model.Other;
import model.Branch;
import model.GenericReceiver;
import model.InheritedReceiver;
import model.GenericMethods;
import model.BinaryReceiver;
// Astral prefix: 🪴. Query and declaration evidence use UTF-8 byte ranges.
public class Use {
    Leaf field;
    static Leaf leaf() { return null; }
    static Target target() { return null; }
    public void run(Target receiver, Leaf leaf, Mid mid, Root root, Other other,
                    Branch branch, Object object, GenericReceiver iterable,
                    InheritedReceiver inherited, GenericMethods generic,
                    BinaryReceiver binary) {
        receiver./*Q:reference_identity*/exact(leaf);
        receiver./*Q:reference_mid_identity*/pick(mid);
        receiver./*Q:reference_root_identity*/pick(root);
        receiver./*Q:reference_unrelated*/pick(other);
        receiver./*Q:reference_multihop*/only(leaf);
        receiver./*Q:reference_specificity*/pick(leaf);
        Target local = receiver;
        Leaf value = leaf;
        local./*Q:reference_local_receiver*/exact(value);
        receiver./*Q:reference_two_arguments*/cross(leaf, leaf);
        receiver./*Q:reference_binary_endpoint*/binary(object);
        receiver./*Q:reference_to_object*/binary(leaf);
        receiver./*Q:reference_interface*/marker(branch);
        receiver./*Q:reference_one_arity*/arity(leaf);
        receiver./*Q:reference_two_arity*/arity(leaf, root);
        receiver./*Q:reference_zero_arity*/empty();
        iterable./*Q:reference_generic_ancestor*/pick(leaf);
        inherited./*Q:reference_inherited_refusal*/pick(leaf);
        generic./*Q:reference_generic_refusal*/pick(leaf);
        binary./*Q:reference_binary_refusal*/pick(leaf);
        receiver./*Q:reference_new_argument_refusal*/pick(new Leaf());
        receiver./*Q:reference_null_refusal*/only(null);
        receiver./*Q:reference_field_refusal*/pick(field);
        receiver./*Q:reference_return_refusal*/pick(leaf());
        new Target()./*Q:reference_new_receiver_refusal*/exact(leaf);
        target()./*Q:reference_receiver_return_refusal*/exact(leaf);
    }
}
