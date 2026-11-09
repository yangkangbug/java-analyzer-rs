// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package ctor;
public class Use { /* UTF-8 boundary: 🧪 */ void run(Target t) throws Exception {
 t./*Q:constructor_identity*/exact(new Leaf());
 t./*Q:constructor_widening*/pick(new Leaf());
 t./*Q:constructor_interface*/marker(new Leaf());
 t./*Q:constructor_annotated*/pick(new Annotated());
 t./*Q:constructor_qualified*/pick(new ctor.Leaf());
 t./*Q:constructor_implicit*/pick(new Default());
 t./*Q:constructor_additional*/pick(new Extra());
 t./*Q:constructor_throws*/pick(new Throwing());
 t./*Q:constructor_generic*/pick(new Generic<String>());
 t./*Q:constructor_generic_ctor*/pick(new GenericCtor());
 t./*Q:constructor_varargs*/pick(new Vararg());
 t./*Q:constructor_package_ctor*/pick(new PackageCtor());
 t./*Q:constructor_protected_ctor*/pick(new ProtectedCtor());
 t./*Q:constructor_anonymous*/pick(new Default() {});
 t./*Q:constructor_abstract_anonymous*/pick(new Abstract() {});
 t./*Q:constructor_nested*/pick(new Outer.Nested());
 t./*Q:constructor_arguments*/pick(new Extra(1));
 t./*Q:constructor_array*/array(new Root[0]);
 t./*Q:constructor_specificity*/specific(new Leaf());
} }
